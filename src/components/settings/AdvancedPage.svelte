<script lang="ts">
  import { RefreshCw } from "@lucide/svelte";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { preferences } from "../../lib/api";
  import { refreshSettings } from "../../lib/settings.svelte";
  import { showSnackbar } from "../../lib/snackbar.svelte";
  import { Button } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import { settingsWindow } from "./settings-window.svelte";

  let cliInstalled = $state(false);
  let cliError = $state("");

  $effect(() => {
    preferences
      .checkCliInstalled()
      .then((installed) => (cliInstalled = installed))
      .catch((error) => console.warn("Failed to check the CLI install:", error));
  });

  async function reloadConfig() {
    try {
      await refreshSettings();
      settingsWindow.editorDraft = null;
      showSnackbar("Config reloaded from disk", "success");
    } catch (error) {
      showSnackbar(`Failed to refresh config: ${error}`, "error");
    }
  }

  async function installCli() {
    cliError = "";
    try {
      await preferences.installCli();
      cliInstalled = true;
    } catch (error) {
      cliError = String(error);
    }
  }

  async function openLogFolder() {
    try {
      await revealItemInDir(await preferences.getLogDir());
    } catch (error) {
      showSnackbar(`Failed to open the logs folder: ${error}`, "error");
    }
  }
</script>

<SettingsSection title="Config file" help="Every setting lives in ~/.config/planeai/config.json.">
  <SettingRow id="reload-config">
    <Button type="button" onclick={reloadConfig}><RefreshCw size={12} class="mr-1.5" />Reload</Button>
  </SettingRow>
</SettingsSection>

<SettingsSection title="Command line">
  <SettingRow id="cli">
    {#snippet note()}
      <p class="mt-0.5 text-[12px] leading-snug text-t3">
        {cliInstalled ? "Installed at" : "Creates a symlink at"} <code class="font-mono">/usr/local/bin/planeai-cli</code>
      </p>
      {#if cliError}<p class="mt-1 break-words text-[12px] text-status-exited" role="alert">{cliError}</p>{/if}
    {/snippet}
    <Button type="button" onclick={installCli}>{cliInstalled ? "Reinstall" : "Install"}</Button>
  </SettingRow>
</SettingsSection>

<SettingsSection title="Diagnostics">
  <SettingRow id="logs">
    <Button type="button" onclick={openLogFolder}>Open logs folder</Button>
  </SettingRow>
</SettingsSection>
