<script lang="ts">
  import { CircleAlert, CircleCheck } from "@lucide/svelte";
  import { preferences, type McpStatus } from "../../lib/api";
  import { getSettings, type AppConfig } from "../../lib/settings.svelte";
  import { saveSettings } from "../../lib/save-settings";
  import { DEFAULT_MCP_PORT, parseMcpPort } from "../../lib/mcp-settings";
  import { Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import TextField from "./TextField.svelte";

  const config = $derived(getSettings());
  const enabled = $derived(config.mcp_enabled !== false);
  const port = $derived(config.mcp_port ?? DEFAULT_MCP_PORT);
  let status = $state<McpStatus | null>(null);

  async function refreshStatus() {
    try {
      status = await preferences.getMcpStatus();
    } catch (error) {
      console.warn("Failed to read the MCP server status:", error);
    }
  }

  $effect(() => {
    // Re-read whenever the settings change, including a reset or a reload from disk.
    void [enabled, port];
    void refreshStatus();
  });

  async function save(patch: Partial<AppConfig>) {
    await saveSettings(patch);
    await refreshStatus();
  }

  function setPort(input: HTMLInputElement) {
    const parsed = parseMcpPort(input.value);
    if (parsed === null) {
      input.value = String(port);
      return;
    }
    void save({ mcp_port: parsed === DEFAULT_MCP_PORT ? null : parsed });
  }
</script>

<SettingsSection title="PlaneAI tools" help="Sessions started afterwards get PlaneAI's tools; agents outside PlaneAI keep using planeai-cli.">
  <SettingRow id="mcp-server">
    {#snippet note()}
      {#if status?.state === "listening"}
        <p class="mt-1 flex items-center gap-1.5 text-[11px] text-t3" role="status">
          <CircleCheck size={11} class="shrink-0 text-status-running" />
          <span>Listening on <span class="font-mono">http://127.0.0.1:{status.port}/mcp</span></span>
        </p>
      {:else if status?.state === "failed"}
        <p class="mt-1 flex items-start gap-1.5 text-[11px] text-status-review" role="alert">
          <CircleAlert size={11} class="mt-0.5 shrink-0" />
          <span>{status.reason} Agents cannot reach PlaneAI's tools.</span>
        </p>
      {/if}
    {/snippet}
    <Switch label="Give agents PlaneAI tools" checked={enabled} onCheckedChange={(on) => save({ mcp_enabled: on ? null : false })} />
  </SettingRow>
  {#if enabled}
    <SettingRow id="mcp-port">
      <TextField
        type="number"
        min="1"
        max="65535"
        width="w-24"
        mono
        aria-label="MCP server port"
        value={String(port)}
        onchange={(e) => setPort(e.currentTarget)}
      />
    </SettingRow>
  {/if}
</SettingsSection>
