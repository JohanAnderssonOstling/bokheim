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
  assert.deepEqual(new Set(account.sync_states.map(({ state }) => state)), new Set(["local_only", "synced", "sign_in_required", "different_account", "remote_missing"]));
  assert.deepEqual(new Set(settings.network_transports.map(({ transport }) => transport)), new Set(["offline", "wifi", "cellular", "ethernet", "vpn", "other", "unknown"]));

  const sharedAccessForm = source("../components/auth/AccountAccessForms.svelte");
  for (const action of ["onlogin", "onregister", "onresendverification", "onverifyemail", "onrequestpasswordreset", "onresetpassword"]) {
    assert.match(sharedAccessForm, new RegExp(`\\b${action}\\b`));
  }
  assert.match(source("../components/auth/LoginScreen.svelte"), /AccountAccessForms/);
  assert.match(source("../components/shell/ServerAccountDialog.svelte"), /AccountAccessForms/);

  const nativeAccount = ["authentication.rs", "sessions.rs", "account.rs"]
    .map((file) => source(`../../../../ui/gpui/browser/src/universal/sync/${file}`))
    .join("\n");
  for (const operation of ["request_public_registration", "resend_verification", "verify_email", "request_password_reset", "reset_password", "account_sessions", "revoke_account_session", "revoke_other_account_sessions"]) {
    assert.match(nativeAccount, new RegExp(`\\b${operation}\\b`));
  }

  const accountPage = source("../components/shell/ServerAccountDialog.svelte");
  for (const action of ["onrevokesession", "onrevokeothersessions"]) {
    assert.match(accountPage, new RegExp(`\\b${action}\\b`));
  }
});
