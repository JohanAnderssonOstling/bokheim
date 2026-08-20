<script lang="ts">
  import { ACCOUNT_CONTRACT } from "../../lib/shared/types";
  import PinEntry from "./PinEntry.svelte";
  import FormButton from "../shared/buttons/FormButton.svelte";
  import SecondaryButton from "../shared/buttons/SecondaryButton.svelte";
  import TextButton from "../shared/buttons/TextButton.svelte";

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
    onverifyemail: (email: string, pin: string) => Promise<boolean>;
    onrequestpasswordreset: (email: string) => Promise<boolean>;
    onresetpassword: (token: string, password: string) => Promise<boolean>;
  } = $props();

  type Mode = "login" | "register" | "verify" | "request-reset" | "reset";
  let mode = $state<Mode>("login");
  let email = $state("");
  let password = $state("");
  let token = $state("");
  let localError = $state("");
  let loginFailed = $state(false);

  const byteLength = (value: string) => new TextEncoder().encode(value).length;

  function selectMode(next: Mode) {
    mode = next;
    password = "";
    localError = "";
    loginFailed = false;
  }

  function validatePassword(value: string) {
    const limits = ACCOUNT_CONTRACT.validation;
    return (
      [...value].length >= limits.password_min_characters &&
      byteLength(value) <= limits.password_max_bytes
    );
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    localError = "";
    const cleanEmail = email.trim();
    const cleanToken = token.trim();
    if (mode === "login") {
      loginFailed = false;
      loginFailed = !(await onlogin(cleanEmail, password));
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
      if (
        !new RegExp(`^\\d{${ACCOUNT_CONTRACT.validation.verification_pin_digits}}$`).test(
          cleanToken,
        )
      ) {
        localError = `Enter the ${ACCOUNT_CONTRACT.validation.verification_pin_digits}-digit verification PIN from your email.`;
        return;
      }
      await onverifyemail(cleanEmail, cleanToken);
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
  <form onsubmit={submit}>
    {#if mode === "login"}
      <p>Sign in to synchronize your libraries and preferences.</p>
      <label class="field">
        <span>Email</span>
        <input
          required
          type="email"
          maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes}
          bind:value={email}
          autocomplete="email"
          aria-label="Email"
        />
      </label>
      <label class="field">
        <span>Password</span>
        <input
          required
          type="password"
          bind:value={password}
          autocomplete="current-password"
          aria-label="Password"
        />
      </label>
      <div class="form-actions">
        <FormButton disabled={busy || !email.trim() || !password}>Sign in</FormButton>
        {#if ACCOUNT_CONTRACT.capabilities.public_registration}<SecondaryButton
            disabled={busy}
            onclick={() => selectMode("register")}>Create account</SecondaryButton
          >{/if}
      </div>
      {#if loginFailed && ACCOUNT_CONTRACT.capabilities.password_recovery}<TextButton
          onclick={() => selectMode("request-reset")}>Reset password</TextButton
        >{/if}
    {:else if mode === "register"}
      <p>Create an account, then enter the six-digit verification PIN sent by email.</p>
      <label class="field">
        <span>Email</span>
        <input
          required
          type="email"
          maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes}
          bind:value={email}
          autocomplete="email"
          aria-label="Email"
        />
      </label>
      <label class="field">
        <span>Password</span>
        <input
          required
          type="password"
          bind:value={password}
          autocomplete="new-password"
          aria-label="Password"
        />
      </label>
      <small
        >At least {ACCOUNT_CONTRACT.validation.password_min_characters} characters.</small
      >
      <div class="form-actions">
        <FormButton disabled={busy || !email.trim() || !password}
          >Create account</FormButton
        >
        <SecondaryButton
          disabled={busy}
          onclick={() => selectMode("login")}>Sign in</SecondaryButton
        >
      </div>
    {:else if mode === "verify"}
      <p>
        Enter the six-digit verification PIN from your email. Verification signs you in.
      </p>
      <label class="field">
        <span>Verification PIN</span>
        <PinEntry
          bind:value={token}
          length={ACCOUNT_CONTRACT.validation.verification_pin_digits}
          disabled={busy}
        />
      </label>
      <div class="form-actions">
        <FormButton disabled={busy || !token.trim()}>Verify email</FormButton>
        {#if ACCOUNT_CONTRACT.capabilities.resend_verification}
          <SecondaryButton
            disabled={busy || !email.trim()}
            onclick={() => void onresendverification(email.trim())}
            >Resend PIN</SecondaryButton
          >
        {/if}
      </div>
      <TextButton
        disabled={busy}
        onclick={() => selectMode("login")}>Sign in instead</TextButton
      >
    {:else if mode === "request-reset"}
      <p>
        Request a password-reset token. The response is deliberately the same whether the
        address exists or not.
      </p>
      <label class="field">
        <span>Email</span>
        <input
          required
          type="email"
          maxlength={ACCOUNT_CONTRACT.validation.email_max_bytes}
          bind:value={email}
          autocomplete="email"
          aria-label="Email"
        />
      </label>
      <div class="form-actions">
        <FormButton disabled={busy || !email.trim()}>Send reset token</FormButton>
        <SecondaryButton onclick={() => selectMode("reset")}
          >Enter reset token</SecondaryButton
        >
      </div>
      <TextButton onclick={() => selectMode("login")}>Back to sign in</TextButton>
    {:else}
      <p>Enter the reset token and choose a new password.</p>
      <label class="field">
        <span>Token</span>
        <input
          required
          maxlength={ACCOUNT_CONTRACT.validation.one_time_token_bytes}
          bind:value={token}
          autocomplete="one-time-code"
          spellcheck="false"
        />
      </label>
      <label class="field">
        <span>New password</span>
        <input
          required
          type="password"
          bind:value={password}
          autocomplete="new-password"
          aria-label="New password"
        />
      </label>
      <small
        >At least {ACCOUNT_CONTRACT.validation.password_min_characters} characters.</small
      >
      <div class="form-actions">
        <FormButton disabled={busy || !token.trim() || !password}
          >Set new password</FormButton
        >
        <SecondaryButton onclick={() => selectMode("login")}
          >Sign in instead</SecondaryButton
        >
      </div>
    {/if}
  </form>

  {#if progress}<p
      class="progress"
      aria-live="polite"
    >
      {progress}
    </p>{/if}
  {#if localError || error}<p
      class="error"
      role="alert"
    >
      {localError || error}
    </p>{/if}
</div>

<style>
  .account-access {
    display: grid;
    gap: 0.8rem;
  }
  form {
    display: grid;
    gap: 0.55rem;
  }
  .form-actions {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.55rem;
  }
  form > p {
    margin: 0 0 0.2rem;
    color: var(--color-subtle-text);
    font-size: 0.72rem;
    line-height: 1.45;
  }
  .field {
    display: grid;
    gap: 0.25rem;
  }
  .field span,
  small {
    color: var(--color-subtle-text);
    font-size: 0.7rem;
    font-weight: 700;
  }
  input {
    font: inherit;
  }
  input {
    box-sizing: border-box;
    min-width: 0;
    height: 2.7rem;
    padding: 0 0.7rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    outline: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    font-size: 0.8rem;
  }
  .field:focus-within span {
    color: var(--color-accent);
  }
  .field:focus-within input {
    border-color: var(--color-accent);
  }
  input::placeholder {
    color: var(--color-subtle-text);
    opacity: 1;
  }
  .progress,
  .error {
    margin: 0;
    padding: 0.2rem 0.1rem;
    font-size: 0.72rem;
  }
  .progress {
    color: var(--color-subtle-text);
  }
  .error {
    color: var(--color-danger-text);
    overflow-wrap: anywhere;
  }
</style>
