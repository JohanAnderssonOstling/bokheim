import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { FRONTEND_CONTRACT } from "./bindings.ts";

const source = (relative: string) => readFileSync(new URL(relative, import.meta.url), "utf8");

test("account and settings semantics are shared across both frontends", () => {
  const { account, settings } = FRONTEND_CONTRACT;
  assert.equal(account.navigation_label, "Account");
  assert.equal(settings.navigation_label, "Settings");
  assert.equal(account.validation.password_min_characters, 12);
  assert.equal(account.validation.verification_pin_digits, 6);
  assert.equal(account.validation.one_time_token_bytes, 43);
  assert.deepEqual(new Set(account.sync_states.map(({ state }) => state)), new Set(["local_only", "synced", "sign_in_required", "different_account", "remote_missing"]));
  assert.deepEqual(new Set(settings.network_transports.map(({ transport }) => transport)), new Set(["offline", "wifi", "cellular", "ethernet", "vpn", "other", "unknown"]));

  const sharedAccessForm = source("../components/auth/AccountAccessForms.svelte");
  for (const action of ["onlogin", "onregister", "onresendverification", "onverifyemail", "onrequestpasswordreset", "onresetpassword"]) {
    assert.match(sharedAccessForm, new RegExp(`\\b${action}\\b`));
  }
  assert.match(source("../components/auth/LoginScreen.svelte"), /AccountAccessForms/);
  assert.match(source("../components/shell/ServerAccountDialog.svelte"), /AccountAccessForms/);
  assert.doesNotMatch(sharedAccessForm, /Already have a verification PIN/);
  assert.match(sharedAccessForm, /PinEntry/);
  assert.match(source("../components/auth/PinEntry.svelte"), /inputmode="numeric"/);
  assert.match(sharedAccessForm, /onverifyemail\(cleanEmail, cleanToken\)/);
  assert.match(sharedAccessForm, /verification PIN/);
  assert.doesNotMatch(sharedAccessForm, /Email for resend/);
  assert.doesNotMatch(sharedAccessForm, /<nav aria-label="Account action">/);
  assert.match(sharedAccessForm, /loginFailed = !\(await onlogin/);
  assert.match(sharedAccessForm, /loginFailed && ACCOUNT_CONTRACT\.capabilities\.password_recovery/);

  const nativeAccount = ["authentication.rs", "account.rs"]
    .map((file) => source(`../../../ui/gpui/browser/src/universal/sync/${file}`))
    .join("\n");
  for (const operation of ["request_public_registration", "resend_verification", "verify_email", "request_password_reset", "reset_password"]) {
    assert.match(nativeAccount, new RegExp(`\\b${operation}\\b`));
  }
  assert.doesNotMatch(source("../../../ui/gpui/browser/src/universal/sync/view.rs"), /account-have-verification/);
  assert.match(source("../../../ui/gpui/browser/src/universal/sync/view.rs"), /OtpInput/);
  assert.doesNotMatch(source("../../../ui/gpui/browser/src/universal/sync/view.rs"), /Email for resend/);
  assert.doesNotMatch(source("../../../ui/gpui/browser/src/universal/sync/view.rs"), /account-flow-sign-in/);
  assert.match(source("../../../ui/gpui/browser/src/universal/sync/view.rs"), /self\.login_failed && contract\.capabilities\.password_recovery/);

  const accountPage = source("../components/shell/ServerAccountDialog.svelte");
  for (const hidden of ["Sessions", "Current session", "Other session", "onrevokesession", "onrevokeothersessions"]) {
    assert.doesNotMatch(accountPage, new RegExp(hidden));
  }
});

test("both account pages open with the same verdict and storage copy", () => {
  const { summary } = FRONTEND_CONTRACT.account;
  assert.equal(summary.all_synced_label, "Everything is synced");
  assert.equal(summary.sync_now_label, "Sync now");

  // Neither page may hard-code the summary wording: it is the one line desktop
  // and mobile must agree on, so both read it from the contract.
  const accountPage = source("../components/shell/ServerAccountDialog.svelte");
  const nativePage = source("../../../ui/gpui/browser/src/universal/sync/view.rs");
  for (const label of Object.keys(summary)) {
    assert.match(accountPage, new RegExp(`\\b${label}\\b`), `mobile account page does not use summary.${label}`);
    assert.match(nativePage, new RegExp(`\\b${label}\\b`), `desktop account page does not use summary.${label}`);
  }
  // Prose in comments may quote the copy; only the code is checked.
  const withoutComments = (text: string) => text.split("\n").filter((line) => !/^\s*(\/\/|\/\*|\*|<!--)/.test(line)).join("\n");
  for (const literal of Object.values(summary)) {
    assert.doesNotMatch(withoutComments(accountPage), new RegExp(`"${literal}"`), `mobile account page hard-codes "${literal}"`);
    assert.doesNotMatch(withoutComments(nativePage), new RegExp(`"${literal}"`), `desktop account page hard-codes "${literal}"`);
  }

  // The verdict itself is derived from one rule, mirrored in the two languages.
  assert.match(accountPage, /summarizeSync|allSynced/);
  assert.match(nativePage, /LibrarySyncSummary::from_statuses/);
});
