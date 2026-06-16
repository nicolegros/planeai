import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://nicolegros.github.io",
  base: "/planeai",
  integrations: [
    starlight({
      title: "planeai",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/nicolegros/planeai",
        },
      ],
      sidebar: [
        { label: "Getting Started", slug: "getting-started" },
        {
          label: "Guides",
          items: [
            { label: "Configuration", slug: "guides/configuration" },
            { label: "Theming", slug: "guides/theming" },
            { label: "Auto-Dispatch", slug: "guides/auto-dispatch" },
          ],
        },
        {
          label: "Tutorials",
          items: [
            { label: "Your First Session", slug: "tutorials/first-session" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "API Reference", slug: "reference/api" },
          ],
        },
      ],
    }),
  ],
});
