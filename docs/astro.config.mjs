import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://nicolegros.github.io",
  base: "/planeai",
  integrations: [
    starlight({
      title: "planeai",
      logo: {
        src: "./src/assets/logo.png",
        alt: "planeai",
      },
      customCss: ["./src/styles/custom.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/nicolegros/planeai",
        },
      ],
      sidebar: [
        { label: "Getting Started", slug: "getting-started" },
        { label: "Concepts", slug: "concepts" },
        {
          label: "Guides",
          items: [
            { label: "Configuration", slug: "guides/configuration" },
            { label: "Task Management", slug: "guides/task-management" },
            { label: "Auto-Dispatch", slug: "guides/auto-dispatch" },
            { label: "Loops", slug: "guides/loops" },
            { label: "Theming", slug: "guides/theming" },
            { label: "Plugin UI contributions", slug: "guides/plugin-ui-contributions" },
          ],
        },
        {
          label: "Tutorials",
          items: [
            { label: "Your First Session", slug: "tutorials/first-session" },
            { label: "Writing Your First Loop", slug: "tutorials/first-loop" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI Reference", slug: "reference/cli" },
            { label: "API Reference", slug: "reference/api" },
            { label: "Loops Reference", slug: "reference/loops" },
          ],
        },
      ],
    }),
  ],
});
