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
    primary: "bg-surface-900 text-surface-50 hover:bg-surface-800 dark:bg-surface-50 dark:text-surface-900 dark:hover:bg-surface-200",
    ghost: "border border-surface-300 text-surface-700 hover:bg-surface-100 dark:border-surface-600 dark:text-surface-300 dark:hover:bg-surface-800",
    danger: "text-error-600 border border-error-300 hover:bg-error-50 dark:text-error-400 dark:border-error-700 dark:hover:bg-error-950",
  };

  const sizes = {
    sm: "px-2 py-1 text-xs",
    default: "px-3 py-1.5 text-sm",
  };
</script>

<button class="{base} {variants[variant]} {sizes[size]} {className}" {...rest}>
  {@render children()}
</button>
