# Publishing the VS Code extension

Maintainer checklist. This file is not packaged into the `.vsix`.

The extension is published as `tumr.tumr-vscode` (the `publisher` and `name` fields of
`package.json`), Windows x64 only (`--target win32-x64`), marked **Preview** while `"preview": true`.

## One-time setup (owner)

1. **Visual Studio Marketplace publisher**
   - Sign in at https://marketplace.visualstudio.com/manage with the Microsoft account that will own the extension.
   - Create a publisher with the ID `tumr`. If that ID is taken, pick another and change
     `publisher` in `package.json` (and the extension ID in `docs/user/vscode.md` and the website).
2. **Trusted Publishing for the Marketplace** (no token: Azure DevOps personal access tokens are
   retired on 1 December 2026)
   - On the publisher page, open **Trusted Publishing** › **Create** and enter: repository owner
     `Jishnu2608`, repository `TUMR`, workflow file `publish-vscode.yml`, environment
     empty. The workflow asks GitHub for a short-lived identity token (`id-token: write`) and
     `vsce publish --oidc` (vsce 4.1.0 or later, pinned in `package.json`) exchanges it for a
     Marketplace credential that lasts 15 minutes. Nothing is stored in the repository.
   - If the repository is ever made private, run the workflow within 48 hours of creating the policy.
3. **Open VSX** (used by VSCodium, Cursor, Windsurf and others)
   - Sign in at https://open-vsx.org with GitHub, sign the Eclipse publisher agreement, create an access token.
   - Create the namespace once: `npx ovsx create-namespace tumr -p <token>` from `apps/vscode`.
4. **GitHub secret** (*Settings › Secrets and variables › Actions*): `OVSX_PAT` (the Open VSX token).
   A `VSCE_PAT` secret is no longer used; delete it if one was added.
5. **Licence**: Apache-2.0 (`LICENSE` and `NOTICE` in this folder are copies of the ones at the repository root; keep them the same). The Marketplace shows it on the listing.

## Each release

1. Bump `version` in `package.json` and add a `CHANGELOG.md` entry. The Marketplace refuses a version it already has.
2. Check locally: `npm --prefix ../desktop/ui run build`, then `npm run package` and `npm test` in `apps/vscode`.
   Install `dist/tumr-vscode.vsix` (*Extensions › … › Install from VSIX*) and try Run on Emulator.
3. GitHub › Actions › **Publish VS Code extension** › Run workflow:
   - first with **dry run** on (builds, tests, uploads the `.vsix` as an artifact, publishes nothing);
   - then with dry run off and the store: `both`, `marketplace` or `openvsx`. The workflow first
     checks that `CHANGELOG.md` has an entry for the version and that the Marketplace does not
     have that version yet, and stops with a clear message if either is wrong.
4. After the first publish, in `webapp/frontend/lib/site.ts` set `site.vscode.marketplace.published` and
   `site.vscode.openVsx.published` to `true` with the real listing URLs, so the website links to the stores.
5. When the extension is stable, remove `"preview": true`.

Publishing from this PC instead of GitHub (fallback): `npm run publish:marketplace` uses
`vsce publish --azure-credential`, which signs in with Microsoft Entra ID through the Azure CLI
(`az login` first); that identity must be added to the publisher as a member. Microsoft documents
this path for work accounts; with a personal Microsoft account the GitHub workflow is the reliable
route. For Open VSX set `OVSX_PAT` in the shell and run `npm run publish:openvsx`. Always
`npm run package` first.

## What the release build does

`scripts/release.ps1` packages the same `.vsix` into each runtime release
(`tumr-vscode-<version>-win32-x64.vsix`, kind `vscode` in `releases.json` and in `SHA256SUMS`),
so people can install it from the download page before or without the stores. `-SkipVscode` leaves it out.
