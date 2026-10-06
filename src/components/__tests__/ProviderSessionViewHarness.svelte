<script lang="ts">
  import ProviderSessionView from "../ProviderSessionView.svelte";
  import type { SettingsLocation } from "../../lib/settings-registry";
  import type { PluginInventory, Session } from "../../lib/types";

  let { initialInventory, initialSession, onOpenPreferences }: { initialInventory: PluginInventory[]; initialSession: Session; onOpenPreferences: (location: SettingsLocation) => void } = $props();

  // svelte-ignore state_referenced_locally
  let inventory = $state(initialInventory);
  // svelte-ignore state_referenced_locally
  let session = $state(initialSession);

  export function setInventory(next: PluginInventory[]): void {
    inventory = next;
  }

  export function setSession(next: Session): void {
    session = next;
  }
</script>

<ProviderSessionView {session} {inventory} onNavigate={() => {}} {onOpenPreferences} onHandoff={async () => {}} onHandback={async () => {}} />
