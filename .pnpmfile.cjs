// @ts-check
// Override svelte-check's typescript peer dependency to use @typescript/typescript6
// because svelte-check is not yet compatible with TypeScript 7's native Go compiler
// (ts.sys was removed). See: https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/

module.exports = {
  hooks: {
    readPackage(pkg) {
      if (pkg.name === "svelte-check" && pkg.peerDependencies?.typescript) {
        pkg.dependencies = pkg.dependencies || {};
        pkg.dependencies.typescript = "npm:@typescript/typescript6@*";
        delete pkg.peerDependencies.typescript;
      }
      return pkg;
    },
  },
};
