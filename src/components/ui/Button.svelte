<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";

  interface Props extends HTMLButtonAttributes {
    variant?: "primary" | "ghost" | "danger";
    size?: "sm" | "default";
    children: Snippet;
  }

  let { variant = "ghost", size = "default", children, class: className = "", ...rest }: Props = $props();

  const base = "inline-flex items-center justify-center rounded font-medium transition-colors disabled:opacity-50 disabled:pointer-events-none";

  const variants = {
    primary: "bg-accent text-on-accent hover:opacity-90",
    ghost: "border border-border text-t2 hover:bg-panel-hi",
    danger: "text-red-600 border border-red-300 hover:bg-red-50 dark:text-red-400 dark:border-red-700 dark:hover:bg-red-950",
  };

  const sizes = {
    sm: "px-2 py-1 text-xs",
    default: "px-3 py-1.5 text-sm",
  };
</script>

<button class="{base} {variants[variant]} {sizes[size]} {className}" {...rest}>
  {@render children()}
</button>
