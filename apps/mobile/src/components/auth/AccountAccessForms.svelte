<script lang="ts">
  import { ACCOUNT_CONTRACT } from "../../lib/types";
  import PinEntry from "./PinEntry.svelte";

  let {
    busy,
    progress,
    error,
    onlogin,
    onregister,
    onresendverification,
    onverifyemail,
    onrequestpasswordreset,
    onresetpassword,
  }: {
    busy: boolean;
    progress: string;
    error: string;
    onlogin: (email: string, password: string) => Promise<boolean>;
    onregister: (email: string, password: string) => Promise<boolean>;
    onresendverification: (email: string) => Promise<boolean>;
    onverifyemail: (token: string) => Promise<boolean>;
    onrequestpasswordreset: (email: string) => Promise<boolean>;
    onresetpassword: (token: string, password: string) => Promise<boolean>;
  } = $props();

  type Mode = "login" | "register" | "verify" | "request-reset" | "reset";
  let mode = $state<Mode>("login");
  let email = $state("");
  let password = $state("");
  let token = $state("");
  let localError = $state("");

  const byteLength = (value: string) => new TextEncoder().encode(value).length;

  function selectMode(next: Mode) {
    mode = next;
    password = "";
    localError = "";
  }

  function validatePassword(value: string) {
    const limits = ACCOUNT_CONTRACT.validation;
    return [...value].length >= limits.password_min_characters && byteLength(value) <= limits.password_max_bytes;
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    localError = "";
    const cleanEmail = email.trim();
    const cleanToken = token.trim();
    if (mode === "login") {
      await onlogin(cleanEmail, password);
      password = "";
      return;
    }
    if (mode === "register") {
      if (!validatePassword(password)) {
        localError = `Password must contain at least ${ACCOUNT_CONTRACT.validation.password_min_characters} characters.`;
        return;
      }
      if (await onregister(cleanEmail, password)) selectMode("verify");
      return;
    }
    if (mode === "verify") {
      if (!new RegExp(`^\\d{${ACCOUNT_CONTRACT.validation.verification_pin_digits}}$`).test(cleanToken)) {
        localError = `Enter the ${ACCOUNT_CONTRACT.validation.verification_pin_digits}-digit PIN from your email.`;
        return;
      }
      await onverifyemail(cleanToken);
      return;
    }
    if (mode === "request-reset") {
      if (await onrequestpasswordreset(cleanEmail)) selectMode("reset");
      return;
    }
    if (!validatePassword(password)) {
      localError = `Password must contain at least ${ACCOUNT_CONTRACT.validation.password_min_characters} characters.`;
      return;
    }
    if (byteLength(cleanToken) !== ACCOUNT_CONTRACT.validation.one_time_token_bytes) {
      localError = "Enter the complete password-reset token from your email.";
      return;
    }
    if (await onresetpassword(cleanToken, password)) selectMode("login");
  }
</script>

<div class="account-access">
  <nav aria-label="Account action">
    <button type="button" class:active={mode === "login"} onclick={() => selectMode("login")}>Sign in</button>
    {#if ACCOUNT_CONTRACT.capabilities.public_registration}<button type="button" class:active={mode === "register" || mode === "verify"} onclick={() => selectMode("register")}>Create account</button>{/if}
    {#if ACCOUNT_CONTRACT.capabilities.password_recovery}<button type="button" class:active={mode === "request-reset" || mode === "reset"} onclick={() => selectMode("request-reset")}>Reset password</button>{/if}
  </nav>

  <form onsubmit={submit}>
    {#if mode === "login"}
      <p>Sign in to synchronize your libraries and preferences.</p>
      <label><span>Email</span><input required type="email" maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes} bind:value={email} autocomplete="email" /></label>
      <label><span>Password</span><input required type="password" bind:value={password} autocomplete="current-password" /></label>
      <button class="primary" type="submit" disabled={busy || !email.trim() || !password}>Sign in</button>
      <button class="link" type="button" onclick={() => selectMode("request-reset")}>Forgot password?</button>
    {:else if mode === "register"}
      <p>Create an account, then enter the 6-digit PIN sent by email.</p>
      <label><span>Email</span><input required type="email" maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes} bind:value={email} autocomplete="email" /></label>
      <label><span>Password</span><input required type="password" bind:value={password} autocomplete="new-password" /></label>
      <small>At least {ACCOUNT_CONTRACT.validation.password_min_characters} characters.</small>
      <button class="primary" type="submit" disabled={busy || !email.trim() || !password}>Create account</button>
    {:else if mode === "verify"}
      <p>Enter the 6-digit PIN from your email. Verification signs you in.</p>
      <div class="verification-row">
        <PinEntry bind:value={token} length={ACCOUNT_CONTRACT.validation.verification_pin_digits} disabled={busy} />
        <button class="primary" type="submit" disabled={busy || !token.trim()}>Verify email</button>
        {#if ACCOUNT_CONTRACT.capabilities.resend_verification}
          <button class="secondary" type="button" disabled={busy || !email.trim()} onclick={() => void onresendverification(email.trim())}>Resend PIN</button>
        {/if}
      </div>
    {:else if mode === "request-reset"}
      <p>Request a password-reset token. The response is deliberately the same whether the address exists or not.</p>
      <label><span>Email</span><input required type="email" maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes} bind:value={email} autocomplete="email" /></label>
      <button class="primary" type="submit" disabled={busy || !email.trim()}>Send reset token</button>
      <button class="link" type="button" onclick={() => selectMode("reset")}>Already have a reset token?</button>
    {:else}
      <p>Enter the reset token and choose a new password.</p>
      <label><span>Token</span><input required maxlength={ACCOUNT_CONTRACT.validation.one_time_token_bytes} bind:value={token} autocomplete="one-time-code" spellcheck="false" /></label>
      <label><span>New password</span><input required type="password" bind:value={password} autocomplete="new-password" /></label>
      <small>At least {ACCOUNT_CONTRACT.validation.password_min_characters} characters.</small>
      <button class="primary" type="submit" disabled={busy || !token.trim() || !password}>Set new password</button>
    {/if}
  </form>

  {#if progress}<p class="progress" aria-live="polite">{progress}</p>{/if}
  {#if localError || error}<p class="error" role="alert">{localError || error}</p>{/if}
</div>

<style>
  .account-access { display: grid; gap: .8rem; }
  nav { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 1px; overflow: hidden; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-border); }
  nav button { min-width: 0; min-height: 2.3rem; padding: .35rem; border: 0; color: var(--color-subtle-text); background: var(--color-raised-bg); font-size: .68rem; font-weight: 750; }
  nav button.active { color: var(--color-raised-bg); background: var(--color-accent); }
  form { display: grid; gap: .55rem; }
  form > p { margin: 0 0 .15rem; color: var(--color-subtle-text); font-size: .72rem; line-height: 1.5; }
  label { display: grid; grid-template-columns: 6rem minmax(0, 1fr); align-items: center; min-height: 2.8rem; padding: 0 .7rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-raised-bg); }
  label:focus-within { border-color: var(--color-accent); }
  label span, small { color: var(--color-subtle-text); font-size: .7rem; font-weight: 700; }
  input, button { font: inherit; }
  input { min-width: 0; height: 2.7rem; border: 0; outline: 0; color: inherit; background: transparent; font-size: .8rem; }
  button { cursor: pointer; }
  button:disabled { opacity: .55; cursor: default; }
  .primary, .secondary { min-height: 2.65rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); font-size: .75rem; font-weight: 750; }
  .primary { color: var(--color-raised-bg); background: var(--color-accent); }
  .secondary { color: var(--color-accent); background: transparent; }
  .link { justify-self: start; padding: .25rem 0; border: 0; color: var(--color-accent); background: transparent; font-size: .7rem; font-weight: 750; }
  .progress, .error { margin: 0; padding: .2rem .1rem; font-size: .72rem; }
  .progress { color: var(--color-subtle-text); }
  .error { color: var(--color-danger-text); overflow-wrap: anywhere; }
  .verification-row { display: grid; grid-template-columns: minmax(0, 1fr) repeat(2, minmax(6rem, .55fr)); align-items: center; gap: .55rem; }
  .verification-row button { box-sizing: border-box; width: 100%; height: 2.65rem; min-height: 2.65rem; }
  @media (max-width: 24rem) { nav { grid-template-columns: 1fr; } label { grid-template-columns: 1fr; padding-top: .4rem; } }
</style>
