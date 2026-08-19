import { mount, unmount } from "svelte";
import ProjectForm from "../components/ProjectForm.svelte";
import * as projectStore from "./project-store.svelte";
import type { Project } from "./types";

export interface PluginModalControls {
  close(): void;
  setSubmitting(submitting: boolean): void;
}

export interface PluginModalOptions {
  title: string;
  mount(root: ShadowRoot, controls: PluginModalControls): (() => void) | void;
}

type ManagedModal = {
  layer: HTMLDivElement;
  dialog: HTMLDivElement;
  body: HTMLDivElement;
  restoreFocus: HTMLElement | null;
  submitting: boolean;
  cleanup: () => void;
};

const stack: ManagedModal[] = [];

function isTop(modal: ManagedModal): boolean {
  return stack.at(-1) === modal;
}

function focusables(dialog: HTMLElement): HTMLElement[] {
  const selector =
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
  const roots: ParentNode[] = [dialog];
  for (const element of dialog.querySelectorAll<HTMLElement>("*")) {
    if (element.shadowRoot) roots.push(element.shadowRoot);
  }
  return roots
    .flatMap((root) => Array.from(root.querySelectorAll<HTMLElement>(selector)))
    .filter((element) => element.offsetParent !== null);
}

function focusModal(modal: ManagedModal): void {
  const [first] = focusables(modal.dialog);
  (first ?? modal.dialog).focus();
}

function closeModal(modal: ManagedModal): void {
  if (!isTop(modal)) return;
  stack.pop();
  modal.cleanup();
  modal.layer.remove();
  const next = stack.at(-1);
  if (next) focusModal(next);
  else modal.restoreFocus?.focus();
}

function trapTab(event: KeyboardEvent, modal: ManagedModal): void {
  if (event.key !== "Tab") return;
  const elements = focusables(modal.dialog);
  if (elements.length === 0) {
    event.preventDefault();
    modal.dialog.focus();
    return;
  }
  const active = document.activeElement as HTMLElement | null;
  const index = elements.indexOf(active ?? elements[0]);
  if (event.shiftKey && index <= 0) {
    event.preventDefault();
    elements.at(-1)?.focus();
  } else if (!event.shiftKey && index === elements.length - 1) {
    event.preventDefault();
    elements[0]?.focus();
  }
}

function openShell(title: string): { modal: ManagedModal; controls: PluginModalControls } {
  const layer = document.createElement("div");
  layer.className = "fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4";
  layer.dataset.pluginModal = "";
  const dialog = document.createElement("div");
  dialog.className =
    "flex max-h-[calc(100vh-2rem)] w-full max-w-md flex-col overflow-hidden rounded-xl border border-border bg-panel shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)]";
  dialog.tabIndex = -1;
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  const heading = document.createElement("h2");
  heading.className = "flex-shrink-0 px-5 pt-5 pb-3 text-[15px] font-semibold text-t1";
  heading.id = `plugin-modal-title-${crypto.randomUUID()}`;
  heading.textContent = title;
  dialog.setAttribute("aria-labelledby", heading.id);
  const body = document.createElement("div");
  body.className = "min-h-0 flex-1 overflow-y-auto overscroll-contain";
  dialog.append(heading, body);
  layer.append(dialog);

  let modal!: ManagedModal;
  const controls: PluginModalControls = {
    close: () => {
      if (!modal.submitting) closeModal(modal);
    },
    setSubmitting: (submitting) => {
      modal.submitting = submitting;
      layer.dataset.submitting = String(submitting);
    },
  };
  modal = {
    layer,
    dialog,
    body,
    restoreFocus: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    submitting: false,
    cleanup: () => {},
  };

  layer.addEventListener("mousedown", (event) => {
    if (event.target === layer && !modal.submitting) controls.close();
  });
  dialog.addEventListener("keydown", (event) => {
    if (!isTop(modal)) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape" && !modal.submitting) {
      event.preventDefault();
      event.stopPropagation();
      controls.close();
      return;
    }
    trapTab(event, modal);
  });
  document.body.append(layer);
  stack.push(modal);
  queueMicrotask(() => focusModal(modal));
  return { modal, controls };
}

/** Opens a host-owned shell with a plugin-owned ShadowRoot body. */
export function openPluginModal(options: PluginModalOptions): PluginModalControls {
  const { modal, controls } = openShell(options.title);
  const root = modal.body.attachShadow({ mode: "open" });
  try {
    modal.cleanup = options.mount(root, controls) ?? (() => root.replaceChildren());
  } catch (error) {
    closeModal(modal);
    throw error;
  }
  return controls;
}

/** Opens the existing host ProjectForm above the active plugin modal. */
export function openProjectForm(): Promise<Project | null> {
  return new Promise((resolve) => {
    const { modal, controls } = openShell("Add Project");
    let component: ReturnType<typeof mount> | null = null;
    const finish = (project: Project | null) => {
      if (!isTop(modal)) return;
      closeModal(modal);
      resolve(project);
    };
    component = mount(ProjectForm, {
      target: modal.body,
      props: {
        onCreated: async (project: Project) => {
          try {
            await projectStore.loadProjects();
          } finally {
            finish(project);
          }
        },
        onCancel: () => finish(null),
      },
    });
    modal.cleanup = () => {
      if (component) unmount(component);
      component = null;
    };
    // ProjectForm handles its own normal/insert escape behavior. The host shell
    // remains responsible for backdrop, tab trapping, and final focus restoration.
    void controls;
  });
}
