# Publishing the VS Code extension

Maintainer checklist. This file is not packaged into the `.vsix`.

The extension is published as `ai-mobile-runtime.umr-vscode` (the `publisher` and `name` fields of
`package.json`), Windows x64 only (`--target win32-x64`), marked **Preview** while `"preview": true`.

## One-time setup (owner)

1. **Visual Studio Marketplace publisher**
   - Sign in at https://marketplace.visualstudio.com/manage with the Microsoft account that will own the extension.
   - Create a publisher with the ID `ai-mobile-runtime`. If that ID is taken, pick another and change
     `publisher` in `package.json` (and the extension ID in `docs/user/vscode.md` and the website).
2. **Azure DevOps token for the Marketplace**
   - At https://dev.azure.com create an organisation if you have none, then *User settings › Personal access tokens*.
   - New token: Organisation **All accessible organisations**, scope **Marketplace › Manage**, short expiry.
3. **Open VSX** (used by VSCodium, Cursor, Windsurf and others)
   - Sign in at https://open-vsx.org with GitHub, sign the Eclipse publisher agreement, create an access token.
   - Create the namespace once: `npx ovsx create-namespace ai-mobile-runtime -p <token>` from `apps/vscode`.
4. **GitHub secrets** (*Settings › Secrets and variables › Actions*): `VSCE_PAT` and `OVSX_PAT`.
5. **Licence**: replace `LICENSE.md` with the real licence once it is chosen. The Marketplace shows it on the listing.

## Each release

1. Bump `version` in `package.json` and add a `CHANGELOG.md` entry. The Marketplace refuses a version it already has.
2. Check locally: `npm --prefix ../desktop/ui run build`, then `npm run package` and `npm test` in `apps/vscode`.
   Install `dist/umr-vscode.vsix` (*Extensions › … › Install from VSIX*) and try Run on Emulator.
3. GitHub › Actions › **Publish VS Code extension** › Run workflow:
   - first with **dry run** on (builds, tests, uploads the `.vsix` as an artifact, publishes nothing);
   - then with dry run off and the store: `both`, `marketplace` or `openvsx`.
4. After the first publish, in `webapp/frontend/lib/site.ts` set `site.vscode.marketplace.published` and
   `site.vscode.openVsx.published` to `true` with the real listing URLs, so the website links to the stores.
5. When the extension is stable, remove `"preview": true`.

Publishing from this PC instead of GitHub: set `VSCE_PAT` / `OVSX_PAT` in the shell, then
`npm run package`, `npm run publish:marketplace`, `npm run publish:openvsx`.

## What the release build does

`scripts/release.ps1` packages the same `.vsix` into each runtime release
(`umr-vscode-<version>-win32-x64.vsix`, kind `vscode` in `releases.json` and in `SHA256SUMS`),
so people can install it from the download page before or without the stores. `-SkipVscode` leaves it out.
