# Test LarpCC on your Mac

## 1. Before you start

Wait for the test handoff. It gives you the approved test build, the two exact
provider paths, and the sign-out instructions for each provider.

This test checks that you can sign in, open and sign out of two new, disposable
profiles called `claude-test` and `codex-test`. They keep their files in a new
folder. You use your usual Claude Code and Codex setup only for the two checks
that confirm your existing logins stay the same. Use only your own accounts.

Quit other Claude Code and Codex sessions, including runs in other Terminal
tabs. Do the test in an unrecorded Terminal. Use a fresh private browser window
or a fresh browser profile for each sign-in. Close it when you finish that
provider's test.

After each command, run `echo $?` before running anything else. That prints the
command's exit code. Keep the code and whether an email appeared. Keep account
details private.

## 2. Check your usual accounts (G0)

G0 is the check that your usual logins stay the same. In your normal Terminal
with your usual setup, run these commands. Keep this Terminal for step 8.

```sh
claude auth status
codex login status
```

Privately note which account each command confirms. If a command only says
you're signed in, use the private account check in the handoff, if provided.
If you still can't identify the account, stop and report that you couldn't
verify the baseline.

## 3. Install the test build

**[depends on PR #QUALIFICATION-ROWS]** **[depends on PR #37]**

Use the checkout from the handoff. A checkout is a local copy of the project at
a specific revision. It must be on the `stage/qualify-mac-rows` test branch,
which is still under review, instead of the regular `main` branch.

In a new Terminal, go to that checkout and run the following. Compare the
revision printed by the first command with the one in the handoff. `--dry-run`
previews the installation paths without building or installing anything.

```sh
git rev-parse HEAD
./scripts/install-macos.sh --dry-run
```

If the revision and paths match, run the installer. See the requirements under
**Install on macOS (local, unsigned)** in that checkout's [README](../README.md).
The default locations are `~/.local/bin/larp` and `~/Applications/LarpCC.app`.
The `~` means your Mac home folder. Keep this Terminal for the rest of the test.

```sh
./scripts/install-macos.sh
export PATH="$HOME/.local/bin:$PATH"
command -v larp
```

Check that `command -v larp` points to the file you just installed. If macOS
blocks this self-built app because of quarantine, clear that mark from this app
only. You can wait until step 7 to do this.

```sh
xattr -dr com.apple.quarantine "$HOME/Applications/LarpCC.app"
```

## 4. Check versions

**[depends on PR #35]** **[depends on PR #QUALIFICATION-ROWS]**

Make a private test folder. `LARPCC_HOME` tells LarpCC where to keep its data.
Set it only in this test Terminal. Stop if the first line fails or the printed
path is empty.

```sh
LARPCC_HOME="$(mktemp -d "$HOME/larpcc-mvp-test.XXXXXX")"
echo "$LARPCC_HOME"
export LARPCC_HOME
cd "$LARPCC_HOME"
larp --version
```

Tell LarpCC which two programs to check. Replace the example paths below with
the exact full paths from the handoff. Keep the quotes. This saves the paths in
the test folder so the version check can find them.

```sh
larp cc provider bind claude '/full/path/to/claude'
larp cc provider bind codex '/full/path/to/codex'
larp cc doctor --detect-versions
```

Both provider versions must match the README's **qualified for** line. That line
lists the exact versions the build allows. Stop if a version differs or is
missing. Report the result before trying any account commands.

## 5. Create the test profiles

```sh
larp cc profile add claude-test --provider claude
larp cc profile add codex-test --provider codex
larp cc profile list
```

Check that the list contains only the two new profiles with the correct
providers. They use the paths you set in step 4.

## 6. Test one profile at a time

**[depends on PR #QUALIFICATION-ROWS]**

Before signing in, read the test build README's sign-out instructions for that
profile. Keychain is where macOS can save sign-in details. If the README says
the login uses Keychain, it must tell you how to remove the exact test item.
Stop before signing in if any required command or sign-out step is missing.

Start with the command below. `NAME` holds the profile name for the eight
checks. Once finished, run `NAME=codex-test` and repeat the checks.

```sh
NAME=claude-test
```

**If you started sign-in and anything fails, times out, or is cancelled, go
straight to checks 7 and 8.** If sign-out fails or you can't confirm `Signed out`,
stop, report it and keep both the profile and the test folder.

1. Show the profile details. Expect no account details.

   ```sh
   larp cc status "$NAME" --no-account
   ```

2. Check that it says `Signed out`. Stop before signing in if it says anything
   else.

   ```sh
   larp cc status "$NAME"
   ```

3. Start sign-in and finish it in the fresh browser window for this test.

   ```sh
   larp cc login "$NAME"
   ```

4. Check for `Signed in`. If you see the account, confirm privately that it is
   the one you chose. If the output gives you no way to identify it, record that
   separately from the sign-in result.

   ```sh
   larp cc status "$NAME"
   ```

5. Open the profile. Quit as soon as it opens, before sending a prompt.

   ```sh
   larp cc run "$NAME"
   ```

6. Check that it still says `Signed in`. If an account is shown, check that it
   matches check 4.

   ```sh
   larp cc status "$NAME"
   ```

7. **Quit and confirm that every Claude Code or Codex window and process from
   this test has stopped.** Use Activity Monitor if needed. If you can't
   confirm this, stop and report it. Then sign out. Follow any README Keychain
   step for the exact test item. If the README gives a Keychain action as your
   sign-out method, use that action in place of the command. Keep any item you
   can't identify with certainty.

   ```sh
   larp cc logout "$NAME"
   ```

8. Confirm `Signed out` before moving on. Keep the profile for now. If the check
   fails, report it and leave the profile and test folder in place.

   ```sh
   larp cc status "$NAME"
   ```

## 7. Check the app, menu bar and a shortcut command

If the handoff includes local usage, check the numbers recorded on this Mac.
If no usage was recorded, expect an unavailable or empty display. Opening and
quitting a profile can leave no numbers to show.
**[depends on PR #USAGE-CLI]**

```sh
larp cc usage --local
```

If the handoff includes the Mac app and gives a way to point it at the test
folder, open `LarpCC.app`. In **General → larp executable → Choose…**, select
the installed `larp`. Confirm the app shows the two test profiles and check its
usage display against the command above. If the menu bar is included, open the
LarpCC icon at the top of the screen and make the same checks. If either view
can't use the test folder or shows other profiles, close it and mark that check
SKIPPED. Quit LarpCC when done.
**[depends on PR #37]** **[depends on PR #USAGE-APP]**
**[depends on PR #19]** **[depends on PR #USAGE-MENU-BAR]**

An alias is a shortcut command for a profile. Use the following to check that
`mvp-codex-test` points to `codex-test` and opens Codex. Run this only after
Codex passed the signed-out check, and quit right away without signing in. Then
confirm it still shows `Signed out`. **[depends on PR #QUALIFICATION-ROWS]**

```sh
larp cc alias add mvp-codex-test codex-test
larp cc alias list
"$LARPCC_HOME/bin/mvp-codex-test"
larp cc status codex-test
```

## 8. Re-check your usual accounts (G0)

In the same normal Terminal you used in step 2, repeat the two checks and any
private account check you used there.

```sh
claude auth status
codex login status
```

Privately confirm that each shows the same account it showed in step 2. If
either differs or you can't confirm a match, stop and report **G0 FAILED**
immediately. Keep the test folder for follow-up.

## 9. Clean up

Return to the test Terminal. Only delete a profile after its test processes have
stopped and you confirmed `Signed out`. Deleting the Codex profile also removes
its shortcut command. If a test stopped early, keep any profile whose signed-out
state you couldn't confirm.

```sh
larp cc profile delete claude-test --yes
larp cc profile delete codex-test --yes
```

Once both deletes succeed, run `echo "$LARPCC_HOME"` to see the exact test folder.
Move that `larpcc-mvp-test.*` folder to the Trash in Finder and close the test
Terminal. Leave your usual LarpCC folder alone.

## 10. Send your results

Use one line per command or check, including skipped checks. Copy the profile
lines for each profile and the other lines as needed. For a check without a
command, use `exit=n/a`. Share only the result, exit code and whether an email
appeared. Keep emails, plan text, screenshots of account pages, tokens, sign-in
URLs and raw command output private. The two version lines are safe to include.

```text
larp --version:
README "qualified for" line:
G0 result (PASS / G0 FAILED / SKIPPED):

Step/command | PASS/FAIL/SKIPPED | exit=___ | email shown=yes/no
1 preparation |
2 baseline (one line per provider) |
3 checkout/preview/install (one line per command) |
4 test folder/bind/version (one line per command) |
5 add/list (one line per command) |
6 claude-test (one line per check, 1–8) |
6 codex-test (one line per check, 1–8) |
7 usage/app/menu bar/alias (one line per check or command) |
8 G0 re-check (one line per provider) |
9 cleanup (one line per command or check) |
10 report |
11 uninstall, if done |
```

## 11. Uninstall the test build

**[depends on PR #37]**

Follow **Uninstall** in the test build's [README](../README.md). Quit LarpCC,
remove the `larp` file installed in step 3 and move `~/Applications/LarpCC.app`
to the Trash. If you added its folder to your Terminal settings, undo that edit.
Your usual Claude Code and Codex setups are designed to stay untouched. Step 8
checks that their logins stayed the same.

Keep any folder you still need for sign-out or recovery. Removing a LarpCC data
folder removes all LarpCC profiles and settings in that folder. Only remove it
if you mean to erase that data. On Mac, the usual folder is
`~/Library/Application Support/larpcc`. A set `LARPCC_HOME` overrides it.
`~/.local/share/larpcc` is the usual Linux location.
