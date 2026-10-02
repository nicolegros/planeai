<script lang="ts">
  import { Pencil, Plus, Trash2 } from "@lucide/svelte";
  import { getSettings, updateSettings, type EditorSettings, type LanguageServerProfile } from "../../lib/settings.svelte";
  import { EDITOR_PRESETS, editorDraft, validateEditorSettings, type EditorMode } from "../../lib/editor-settings";
  import {
    emptyLanguageServerProfileDraft,
    languageServerProfileDraft,
    validateLanguageServerProfile,
    type LanguageServerProfileDraft,
  } from "../../lib/language-server-profile";
  import { Button, Dialog, SegmentedControl, Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import TextField from "./TextField.svelte";

  const config = $derived(getSettings());
  const lspEnabled = $derived(config.language_servers?.enabled ?? true);
  const profiles = $derived(config.language_servers?.profiles ?? []);

  // The editor command is validated as a whole, so it is edited as a draft and saved explicitly.
  // svelte-ignore state_referenced_locally
  let draft = $state<EditorSettings>(editorDraft(config.editor));
  let draftError = $state("");

  let profileDialogOpen = $state(false);
  let editingProfileId = $state<string | null>(null);
  let profileDraft = $state<LanguageServerProfileDraft>(emptyLanguageServerProfileDraft());
  let profileError = $state("");

  function setMode(mode: EditorMode) {
    draft = editorDraft(draft, mode);
    draftError = "";
  }

  function saveEditor() {
    const error = validateEditorSettings(draft);
    if (error) {
      draftError = error;
      return;
    }
    draftError = "";
    void updateSettings({ editor: draft.mode === "embedded" ? null : { ...draft, command: draft.command.trim() } });
  }

  function setLanguageServers(next: { enabled?: boolean; profiles?: LanguageServerProfile[] }) {
    void updateSettings({
      language_servers: { ...config.language_servers, enabled: next.enabled ?? lspEnabled, profiles: next.profiles ?? profiles },
    });
  }

  function openProfile(profile?: LanguageServerProfile) {
    editingProfileId = profile?.id ?? null;
    profileDraft = profile ? languageServerProfileDraft(profile) : emptyLanguageServerProfileDraft();
    profileError = "";
    profileDialogOpen = true;
  }

  function saveProfile() {
    const result = validateLanguageServerProfile(profileDraft, profiles.map((profile) => profile.id), editingProfileId ?? undefined);
    if (!result.ok) {
      profileError = result.error;
      return;
    }
    setLanguageServers({
      profiles: editingProfileId ? profiles.map((profile) => (profile.id === editingProfileId ? result.profile : profile)) : [...profiles, result.profile],
    });
    profileDialogOpen = false;
  }
</script>

<SettingsSection title="Opening files" help="Terminal and external commands receive absolute {'{file}'}, active-worktree {'{project}'} and PlaneAI {'{session_id}'} placeholders.">
  <SettingRow id="file-editor" stacked>
    <SegmentedControl
      name="file-editor-mode"
      label="Open files in"
      options={[{ value: "embedded", label: "Embedded" }, { value: "terminal", label: "Terminal command" }, { value: "external", label: "External app" }]}
      value={draft.mode}
      onValueChange={setMode}
    />
    {#if draft.mode !== "embedded"}
      <div class="mt-3 space-y-3 rounded-lg border border-border p-3">
        <div class="flex flex-wrap gap-1.5">
          {#each EDITOR_PRESETS as preset (preset.label)}
            <button
              type="button"
              class="rounded-md border border-border-s px-2 py-0.5 text-[12px] text-t2 hover:bg-panel-hi hover:text-t1"
              onclick={() => {
                draft = { ...draft, command: preset.command, args: [...preset.args] };
                draftError = "";
              }}
            >{preset.label}</button>
          {/each}
        </div>
        <div class="space-y-1">
          <label class="text-[12px] text-t2" for="editor-command">Executable</label>
          <TextField id="editor-command" mono width="w-full" value={draft.command} placeholder="e.g. code or nvim" onchange={(e) => { draft.command = e.currentTarget.value; draftError = ""; }} />
        </div>
        <div class="space-y-1">
          <label class="text-[12px] text-t2" for="editor-args">Arguments (one per line)</label>
          <textarea
            id="editor-args"
            class="min-h-20 w-full rounded-md border border-border-s bg-panel px-2 py-1.5 font-mono text-[12px] text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent"
            value={draft.args.join("\n")}
            placeholder={"--goto\n{file}"}
            oninput={(e) => { draft.args = e.currentTarget.value.split("\n").filter(Boolean); draftError = ""; }}
          ></textarea>
        </div>
      </div>
    {/if}
    {#if draftError}<p class="mt-2 text-[12px] text-status-exited" role="alert">{draftError}</p>{/if}
    <div class="mt-3"><Button type="button" onclick={saveEditor}>Save editor settings</Button></div>
  </SettingRow>
</SettingsSection>

<SettingsSection title="Keybindings">
  <SettingRow id="vim-mode">
    <Switch label="Vim keybindings" checked={config.vim_mode ?? true} onCheckedChange={(vim_mode) => updateSettings({ vim_mode })} />
  </SettingRow>
</SettingsSection>

<SettingsSection title="Language servers" help="Built in: TypeScript/JavaScript/JSON, Rust, Python, Go and C/C++. PlaneAI never installs a server or runs repository-provided commands.">
  <SettingRow id="language-servers">
    <Switch label="Code intelligence" checked={lspEnabled} onCheckedChange={(enabled) => setLanguageServers({ enabled })} />
  </SettingRow>
  <div id="setting-language-server-profiles" data-setting-id="language-server-profiles">
    <div class="flex items-center justify-between gap-6 px-4 py-3">
      <div class="min-w-0">
        <p class="text-[13px] text-t1">Custom profiles</p>
        <p class="text-[12px] leading-snug text-t3">Trusted commands from your PlaneAI config that override discovery for file extensions. Apply to newly opened files.</p>
      </div>
      <Button type="button" size="sm" onclick={() => openProfile()}><Plus size={12} class="mr-1" />Add profile</Button>
    </div>
    {#each profiles as profile (profile.id)}
      <div class="flex items-center gap-3 border-t border-border px-4 py-2.5">
        <div class="min-w-0 flex-1">
          <p class="truncate text-[13px] text-t1">{profile.id}</p>
          <p class="truncate font-mono text-[11px] text-t3">{profile.command}{profile.args?.length ? ` ${profile.args.join(" ")}` : ""}</p>
          <p class="text-[11px] text-t3">{profile.language_id} · {(profile.extensions ?? []).map((extension) => `.${extension}`).join(", ")}</p>
        </div>
        <button type="button" class="grid h-6 w-6 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-t1" aria-label="Edit {profile.id}" onclick={() => openProfile(profile)}><Pencil size={12} /></button>
        <button type="button" class="grid h-6 w-6 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-status-exited" aria-label="Remove {profile.id}" onclick={() => setLanguageServers({ profiles: profiles.filter((candidate) => candidate.id !== profile.id) })}><Trash2 size={12} /></button>
        <Switch
          label="Enable {profile.id} language server"
          checked={profile.enabled !== false}
          onCheckedChange={(enabled) => setLanguageServers({ profiles: profiles.map((candidate) => (candidate.id === profile.id ? { ...candidate, enabled } : candidate)) })}
        />
      </div>
    {/each}
  </div>
</SettingsSection>

<Dialog
  open={profileDialogOpen}
  onOpenChange={(open) => (profileDialogOpen = open)}
  title={editingProfileId ? "Edit language-server profile" : "Add language-server profile"}
  class="w-[34rem] p-5"
>
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); saveProfile(); }}>
    <div>
      <h2 class="text-[15px] font-semibold text-t1">{editingProfileId ? "Edit language-server profile" : "Add language-server profile"}</h2>
      <p class="mt-1 text-[12px] text-t3">Commands are trusted and run only from your PlaneAI configuration, never from a repository.</p>
    </div>
    <div class="grid grid-cols-2 gap-3">
      <div class="space-y-1">
        <label class="text-[12px] text-t2" for="lsp-profile-id">Profile ID</label>
        <TextField id="lsp-profile-id" mono width="w-full" bind:value={profileDraft.id} placeholder="local-rust-analyzer" />
      </div>
      <div class="space-y-1">
        <label class="text-[12px] text-t2" for="lsp-profile-language">Language ID</label>
        <TextField id="lsp-profile-language" mono width="w-full" bind:value={profileDraft.languageId} placeholder="rust" />
      </div>
    </div>
    <div class="space-y-1">
      <label class="text-[12px] text-t2" for="lsp-profile-extensions">File extensions</label>
      <TextField id="lsp-profile-extensions" mono width="w-full" bind:value={profileDraft.extensions} placeholder="rs, rsi" />
      <p class="text-[11px] text-t3">Comma-separated, without the leading dot.</p>
    </div>
    <div class="space-y-1">
      <label class="text-[12px] text-t2" for="lsp-profile-command">Command</label>
      <TextField id="lsp-profile-command" mono width="w-full" bind:value={profileDraft.command} placeholder="rust-analyzer" />
    </div>
    <div class="space-y-1">
      <label class="text-[12px] text-t2" for="lsp-profile-args">Arguments</label>
      <textarea id="lsp-profile-args" bind:value={profileDraft.args} class="min-h-20 w-full resize-y rounded-md border border-border-s bg-panel px-2 py-1.5 font-mono text-[12px] text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent" placeholder="--stdio"></textarea>
      <p class="text-[11px] text-t3">One argument per line, so arguments containing spaces are preserved.</p>
    </div>
    <label class="flex items-center gap-2 text-[13px] text-t1">
      <input type="checkbox" bind:checked={profileDraft.enabled} class="rounded border-border text-accent focus:ring-accent" />
      Enable this profile
    </label>
    {#if profileError}<p class="text-[12px] text-status-exited" role="alert">{profileError}</p>{/if}
    <div class="flex justify-end gap-2 pt-1">
      <Button type="button" onclick={() => (profileDialogOpen = false)}>Cancel</Button>
      <Button type="submit" variant="primary">Save profile</Button>
    </div>
  </form>
</Dialog>
