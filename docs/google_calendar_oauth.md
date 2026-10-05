# Google Calendar by OAuth

Status: **built 2026-10-05.** Automated tests pass. Signed in from the dev app with the school account the same day: 193 events stored, the token in the Keychain and not in the database, and a restart synced again with no new sign-in. Not yet in the everyday app, and nobody has looked at the schedule list on screen after the switch. The spike that came first passed the same day.

## Why

The app reads Google Calendar through a secret iCal address or an uploaded .ics file. The school account has no secret address, and a file goes stale until a new one is uploaded. Signing in with Google lets the calendar update by itself.

## What the spike showed

The question was whether the school's admin blocks third-party apps from the calendar. A throwaway script, `spike/spike.py`, signed in with the school account on 2026-10-05 and read the primary calendar.

- Read-only access is allowed on the school account.
- Google gave a refresh token, so the app can sync without a sign-in each time.
- Every event carried an iCal UID.
- Not tried: calendars other than primary, and a refresh token older than 7 days.

Separately, a dummy value was written to the macOS Keychain through the `security` tool's standard input, read back and deleted. There was no password prompt, and the value never appeared as a command argument.

## Decisions

| Question | Answer |
| --- | --- |
| Who signs in | Only the developer, on this laptop, for now |
| Cloud project status | Stays in "Testing" (decided 2026-10-05), so the sign-in is repeated every 7 days |
| Everyday app | Reinstalled with this on 2026-10-05, with its own copy of `google_client.json`. It still uses the uploaded file until someone signs in there |
| Replace the address and file routes? | No. An account is a third way to connect the Google source. A source holds one of address, file or account at a time |
| Which calendars | Primary only |
| Where the refresh token lives | macOS Keychain, through the built-in `security` tool |
| Scope | `https://www.googleapis.com/auth/calendar.readonly`, the one the spike proved |
| New dependencies | None, in Python or Rust |

The Keychain route was chosen over a Rust keychain library because the app is unsigned: a library would tie the token to the app's signature, and macOS would ask for the login password after every rebuild and reinstall. Its limit is that any program running as the user can ask `security` for the token. It protects the token on disk, not against other software in the session.

## Design

### What is stored where

| Thing | Where | Notes |
| --- | --- | --- |
| Refresh token | Keychain: service is the app identifier (`com.hiennguyen.pet`, or `com.hiennguyen.pet.dev` for the dev app), account `google_refresh_token` | The dev app and the everyday app each have their own |
| OAuth client | `google_client.json` beside `pet.db`: the project root for the dev app, the app data folder for the everyday app | The file downloaded from the Cloud console, unchanged. Ignored by git. Copied in by hand, once per app |
| "Connected by account" | `setting` table, key `feed_account_google`, value `1` | A marker only. No secret is in the database |
| Access token | Nowhere | The fetcher asks for a fresh one at each sync |

### Signing in

1. The user presses "Sign in with Google" in Settings. The panel calls a new command, `connect_google`.
2. Rust reads `google_client.json` and runs the fetcher with `{"connect": {"google": {"client_id": ..., "client_secret": ...}}}` on standard input.
3. The fetcher opens the browser, listens on `127.0.0.1` at a free port for up to 5 minutes, checks the `state` value, and exchanges the code using PKCE. It replies `{"google": {"ok": {"refresh_token": ...}}}` or `{"google": {"error": ...}}`.
4. Rust writes the token to the Keychain, clears any saved address or file for Google, sets `feed_account_google`, and syncs.

The sync lock is not held while the browser is open, so the half-hourly sync is not blocked by a sign-in nobody finishes.

### Syncing

1. `sync_feeds` reads the token from the Keychain and the client from `google_client.json`, and adds `"accounts": {"google": {"client_id", "client_secret", "refresh_token"}}` to the fetcher's request. It goes in on standard input, like feed addresses.
2. The fetcher trades the refresh token for an access token, then lists the primary calendar's events from one day back to 30 days ahead (the window the ICS route uses), with `singleEvents=true` so Google expands repeating events. It follows `nextPageToken` until the list ends and skips cancelled events.
3. Each event becomes the same row the ICS route produces: `kind` `event`, `source` `google`, times in UTC as `YYYY-MM-DDTHH:MM:SSZ` or a plain date for all-day events, and `uid` set to `iCalUID@start`.
4. The reply goes through `store_feed` and `sync_feed` unchanged. The schedule list does not change.

Because the key matches the ICS route's, switching from an uploaded file to an account updates events in place.

### Settings

- The Google row gains a "Sign in with Google" button beside "Save" and "Upload .ics". The Canvas row does not.
- `feeds` reports `account: true` for a source connected this way. The status line reads "Signed in. Synced at 10:30: 42 events."
- "Disconnect" works as it does now (`set_feed` with an empty address) and also deletes the Keychain item and the marker. It does not revoke access at Google; that is done from the Google account page.

### Failures

| What goes wrong | What the user sees | What happens to data |
| --- | --- | --- |
| The refresh token is expired or revoked | "Google sign-in has expired; sign in again" | Existing events stay, as with any failed sync |
| `google_client.json` is missing or unreadable | A message naming the path it expects | Nothing changes |
| The sign-in is not finished in 5 minutes, or is refused | "Sign-in was not completed" | Nothing changes; a saved address or file is kept |
| The network is down, or Google answers with an error | "could not reach Google" or "the server answered 500" | Existing events stay |
| The Keychain cannot be read or written | "could not reach the Keychain" | Nothing changes |

No error message or log line contains a token, a code or the client secret.

### Tests

Automated, in `backend/tests/test_fetcher.py`, with the network faked as the feed tests do:

- Google's reply becomes the right rows: a timed event, an all-day event, a cancelled event skipped, a reply in two pages.
- The same event gets the same `uid` by this route as by the ICS route.
- A refused refresh token gives the "sign in again" message, with no token in it.
- The sign-in address carries the scope, the PKCE challenge and the `state` value.

Automated, in `lib.rs`:

- A source holds exactly one of address, file or account; saving one clears the others; disconnecting clears all three.
- A token goes into the real Keychain under a test service name, is replaced, read back and deleted; a token that could split the command line is refused.
- A missing client file, and one that is not a "Desktop app" client, each give their message.

By hand, since the browser step cannot be automated:

- Sign in from the dev app, see today's events in the panel, quit and reopen, and confirm it still syncs.

### Left out on purpose

- Calendars other than primary. The roadmap has a separate task for choosing calendars.
- Outlook.
- Moving the Canvas and Google feed addresses into the Keychain. (Done afterwards, the same day, with the same helper.)
- Revoking the token at Google on disconnect.
- Bundling the client into the app for other users.

### Known costs

- While the Cloud project is in "Testing", the refresh token expires after 7 days, so the app asks for a sign-in weekly. Moving the project to "In production" in the console is expected to lift that, at the cost of the "Google hasn't verified this app" page at sign-in. Not yet confirmed.
- The Keychain route is macOS only, like the rest of the app today.
- The client file is copied by hand into two places, one per app.
- The sign-in waits for one browser request. A stray request to the same port ends it early with "Sign-in was not completed"; press the button again.

## Setting up the Cloud project

Needed once. Google renames these pages now and then; if a label differs, pick the nearest match.

1. **Sign in with a personal Gmail account** at <https://console.cloud.google.com>. Do not use the school account here: school accounts often cannot create projects, and the project should outlive the school account anyway.
2. **Create a project.** Project picker at the top, then "New project". Name it `Study Pet`. No organization. Make sure it is the selected project afterwards.
3. **Turn on the Calendar API.** Menu, "APIs & Services", "Library". Search for "Google Calendar API", open it, press "Enable".
4. **Set up the consent screen.** Menu, "APIs & Services", "OAuth consent screen" (it opens "Google Auth Platform"). Press "Get started".
   - App name: `Study Pet`. Support email: your Gmail.
   - Audience: **External**.
   - Contact email: your Gmail.
   - Agree and create.
5. **Add the school account as a test user.** "Audience", then "Test users", "Add users". Enter your school email address and save. The publishing status should read "Testing".
6. **Create the client.** "Clients", "Create client".
   - Application type: **Desktop app**. Name: `Study Pet desktop`.
   - Create, then **Download JSON** from the dialog. The secret is shown only now.
   - For the spike, the file goes to `spike/client_secret.json`. For the app, it will go to `google_client.json` as described above.

Step 4 has an optional page, "Data Access", for listing scopes. It can be skipped: the app asks for its scope itself.

## The spike script

`spike/spike.py` is throwaway and ignored by git, along with the client file beside it. To run it again, from the project root:

    backend/.venv/bin/python spike/spike.py

Sign in with the school account and choose "Continue" on the "Google hasn't verified this app" page.

| What happens | What it means |
| --- | --- |
| The terminal prints `RESULT: it works` and a list of events | Access is allowed |
| The browser shows "Access blocked" with a mention of the administrator or `admin_policy_enforced` | The school blocks unapproved apps |
| The browser shows "Access blocked" with `access_denied` and a mention of test users | Step 5 was missed, or a different account signed in |
| `RESULT: Google answered 403` mentioning the Calendar API | Step 3 was missed |

The dev app's `google_client.json` in the project root is a copy of `spike/client_secret.json`. The `spike/` folder can be deleted once the everyday app has its own copy.
