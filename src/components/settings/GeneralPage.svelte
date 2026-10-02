<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { updater } from "../../lib/api";
  import { getSettings, type AppConfig } from "../../lib/settings.svelte";
  import { showSnackbar } from "../../lib/snackbar.svelte";
  import { checkForUpdates, getSettingsUpdateState, setInstalling } from "../../lib/updater.svelte";
  import { Button, SegmentedControl, Switch } from "../ui";
  import TextField from "./TextField.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import { saveSettings } from "./settings-window.svelte";

  const config = $derived(getSettings());
  const appUpdateState = $derived(getSettingsUpdateState());
  let appVersion = $state<string | null>(null);
  let appVersionError = $state<string | null>(null);
  let installUpdateError = $state<string | null>(null);

  $effect(() => {
    updater.getVersion().then(
      (version) => (appVersion = version),
      (error) => (appVersionError = String(error)),
    );
  });

  async function pickProjectsFolder() {
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: config.projects_base_path ?? undefined });
      if (selected) await saveSettings({ projects_base_path: selected as string });
    } catch (error) {
      showSnackbar(`Failed to set the projects folder: ${error}`, "error");
    }
  }

  async function checkForAppUpdates() {
    installUpdateError = null;
    await checkForUpdates();
  }

  async function installAppUpdate() {
    if (appUpdateState.installing) return;
    installUpdateError = null;
    setInstalling(true);
    try {
      await updater.install();
    } catch (error) {
      installUpdateError = String(error);
      setInstalling(false);
    }
  }
</script>

<SettingsSection title="Projects">
  <SettingRow id="projects-folder">
    <div class="flex items-center gap-2">
      <TextField
        value={config.projects_base_path ?? ""}
        onchange={(e) => saveSettings({ projects_base_path: e.currentTarget.value || null })}
        placeholder="e.g. ~/Developer"
        aria-label="Projects folder"
        mono
      />
      <Button type="button" onclick={pickProjectsFolder}>Choose…</Button>
    </div>
  </SettingRow>
</SettingsSection>

<SettingsSection title="After a merge">
  <SettingRow id="post-merge-action">
    <SegmentedControl
      name="post-merge-action"
      label="If the merge prompt is ignored"
      options={[{ value: "archive", label: "Archive" }, { value: "destroy", label: "Destroy" }, { value: "keep", label: "Keep" }]}
      value={config.post_merge_action ?? "archive"}
      onValueChange={(value) => saveSettings({ post_merge_action: value as AppConfig["post_merge_action"] })}
    />
  </SettingRow>
</SettingsSection>

<SettingsSection title="Review">
  <SettingRow id="auto-open-review">
    <Switch label="Open review when an agent finishes" checked={config.auto_open_review ?? false} onCheckedChange={(checked) => saveSettings({ auto_open_review: checked })} />
  </SettingRow>
</SettingsSection>

<SettingsSection title="Notifications">
  <SettingRow id="sound">
    <Switch label="Play a sound when an agent finishes" checked={config.sound_enabled ?? true} onCheckedChange={(checked) => saveSettings({ sound_enabled: checked })} />
  </SettingRow>
</SettingsSection>

<SettingsSection title="Updates">
  <SettingRow id="app-updates">
    {#snippet note()}
      <div class="mt-0.5 text-[12px] leading-snug">
        {#if appVersion}
          <p class="text-t2">PlaneAI v{appVersion}</p>
        {:else if appVersionError}
          <p class="text-t2">Unable to determine PlaneAI version</p>
          <p class="break-words text-status-exited" role="alert">{appVersionError}</p>
        {:else}
          <p class="text-t3">Loading installed version…</p>
        {/if}
        {#if appUpdateState.checking}
          <p class="text-t3">Checking for updates…</p>
        {:else if appUpdateState.installing}
          <p class="text-t3">Downloading and installing{appUpdateState.updateAvailable ? ` v${appUpdateState.updateAvailable.version}` : ""}…</p>
        {:else if installUpdateError}
          <p class="whitespace-pre-wrap break-words text-status-exited" role="alert">{installUpdateError}</p>
        {:else if appUpdateState.checkError}
          <p class="whitespace-pre-wrap break-words text-status-exited" role="alert">{appUpdateState.checkError}</p>
        {:else if appUpdateState.updateAvailable}
          <p class="text-t3">Update available: v{appUpdateState.updateAvailable.version}</p>
        {:else if appUpdateState.upToDate}
          <p class="text-t3">You’re up to date{appVersion ? ` (v${appVersion})` : ""}.</p>
        {/if}
      </div>
    {/snippet}
    {#if appUpdateState.installing}
      <Button type="button" disabled>Installing…</Button>
    {:else if appUpdateState.updateAvailable && !appUpdateState.checkError}
      <Button type="button" variant="primary" onclick={installAppUpdate}>Install &amp; Restart</Button>
    {:else if appUpdateState.checking}
      <Button type="button" disabled>Checking…</Button>
    {:else if appUpdateState.checkError}
      <Button type="button" onclick={checkForAppUpdates}>Retry</Button>
    {:else if appUpdateState.upToDate}
      <Button type="button" onclick={checkForAppUpdates}>Check again</Button>
    {:else}
      <Button type="button" onclick={checkForAppUpdates}>Check for updates</Button>
    {/if}
  </SettingRow>
</SettingsSection>
