import plexSansLatin from "@fontsource-variable/ibm-plex-sans/files/ibm-plex-sans-latin-wght-normal.woff2?url";
import plexMonoLatin400 from "@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-400-normal.woff2?url";
import plexMonoLatin500 from "@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-500-normal.woff2?url";

/** A font face a local plugin frame registers with `new FontFace(family, data, descriptors)`. */
export interface PluginFrameFont {
  family: string;
  data: ArrayBuffer;
  descriptors: FontFaceDescriptors;
}

const latin =
  "U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD";

/** The latin faces of the @fontsource rules main.ts loads, with their descriptors. */
const faces = [
  { family: "IBM Plex Sans Variable", url: plexSansLatin, weight: "100 700" },
  { family: "IBM Plex Mono", url: plexMonoLatin400, weight: "400" },
  { family: "IBM Plex Mono", url: plexMonoLatin500, weight: "500" },
];

let loading: Promise<PluginFrameFont[]> | undefined;

/**
 * The theme fonts for local plugin frames, which have an opaque origin and a CSP that
 * blocks font fetches. Loaded once and shared by every frame.
 */
export function pluginFrameFonts(): Promise<PluginFrameFont[]> {
  loading ??= Promise.all(
    faces.map(async ({ family, url, weight }) => {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
      return {
        family,
        data: await response.arrayBuffer(),
        descriptors: { style: "normal", weight, unicodeRange: latin, display: "swap" },
      };
    }),
  ).catch((error) => {
    console.error("Plugin frame fonts failed to load", error);
    return [];
  });
  return loading;
}
