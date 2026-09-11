import type { LanguageServerProfile } from "./settings.svelte";

export interface LanguageServerProfileDraft {
  id: string;
  languageId: string;
  extensions: string;
  command: string;
  args: string;
  enabled: boolean;
}

export type ProfileValidation =
  | { ok: true; profile: LanguageServerProfile }
  | { ok: false; error: string };

export function emptyLanguageServerProfileDraft(): LanguageServerProfileDraft {
  return {
    id: "",
    languageId: "",
    extensions: "",
    command: "",
    args: "",
    enabled: true,
  };
}

export function languageServerProfileDraft(
  profile: LanguageServerProfile,
): LanguageServerProfileDraft {
  return {
    id: profile.id,
    languageId: profile.language_id,
    extensions: (profile.extensions ?? []).join(", "),
    command: profile.command,
    args: (profile.args ?? []).join("\n"),
    enabled: profile.enabled !== false,
  };
}

export function validateLanguageServerProfile(
  draft: LanguageServerProfileDraft,
  existingIds: Iterable<string>,
  originalId?: string,
): ProfileValidation {
  const id = draft.id.trim();
  const languageId = draft.languageId.trim();
  const command = draft.command.trim();
  const extensions = draft.extensions
    .split(",")
    .map((extension) => extension.trim().replace(/^\.+/, "").toLowerCase())
    .filter(Boolean);
  const args = draft.args.split("\n").map((arg) => arg.trim()).filter(Boolean);

  if (!id) return { ok: false, error: "Profile ID is required." };
  if (!languageId) return { ok: false, error: "Language ID is required." };
  if (extensions.length === 0) return { ok: false, error: "Add at least one file extension." };
  if (!command) return { ok: false, error: "Language-server command is required." };
  if (Array.from(existingIds).some((existingId) => existingId === id && existingId !== originalId)) {
    return { ok: false, error: `A profile named “${id}” already exists.` };
  }

  return {
    ok: true,
    profile: {
      id,
      language_id: languageId,
      extensions: Array.from(new Set(extensions)),
      command,
      args,
      enabled: draft.enabled,
    },
  };
}
