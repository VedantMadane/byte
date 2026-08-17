# Security policy for byte

## Supported versions

The latest release line on `main` receives security fixes. Older lines are
considered end-of-life.

## Reporting a vulnerability

**Do not open public GitHub issues for security problems.**

Instead, please report privately via [GitHub Security Advisories](https://github.com/lindstrm/byte/security/advisories/new),
or by email to `jocke@indicio.com`.

## Response

We aim to acknowledge reports within 72 hours and provide a triage update
within 7 days.

## Disclosure

We follow coordinated disclosure: we will agree on a release window with the
reporter and credit them in the release notes (unless they request otherwise).

## Scope

In scope: any vulnerability in the published release of byte.
Out of scope: vulnerabilities in third-party dependencies (please report those
upstream).

## Threat model

byte stores each account's OAuth refresh token in the OS credential store
(Windows Credential Manager, the macOS login Keychain, or a Linux Secret
Service provider) so it can switch accounts without repeating a browser
login. These are live credentials, and the honest framing is this: **anyone
who can run code as the logged-in user can read them.**

That is not a new exposure byte introduces. It is equally true today of
Claude Code's own credential file, `~/.claude/.credentials.json`, which is
the source byte copies those tokens from in the first place. byte does not
worsen that posture — but it does not improve it either. It duplicates the
same secret into a second location (one entry per stored account, in the OS
credential store), so there are now two places, not one, where a local
attacker with code-execution-as-you could read a live refresh token. Neither
location adds encryption or access control beyond what the OS credential
store already provides on its own.

If your threat model includes a local attacker able to run arbitrary code as
you, the correct response to a suspected compromise is the same regardless
of which copy was read: revoke the affected account's session from your
claude.ai account settings. `byte remove <name>` deletes byte's copy of the
credential (both its metadata entry and its OS credential store entry), but
neither that nor deleting `~/.claude/.credentials.json` revokes the token
itself — only Anthropic's auth servers can do that.

`accounts.json`, byte's own metadata file, never contains a token — only the
credential store entries do — so reading `accounts.json` alone (e.g. its
contents ending up in a support bundle or backup) does not expose account
credentials.