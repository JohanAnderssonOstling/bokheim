<script lang="ts">
  import { APP_NAME } from "../../../lib/app/application-name";
  import AccountAccessForms from "../../../components/auth/AccountAccessForms.svelte";

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
</script>

<main class="login-screen">
  <section
    class="login-card"
    aria-labelledby="login-title"
  >
    <div class="heading">
      <p>{APP_NAME}</p>
      <h1 id="login-title">Sign in to your library</h1>
      <span>Use one account for all devices and keep your libraries in sync.</span>
    </div>

    <AccountAccessForms
      {busy}
      {progress}
      {error}
      {onlogin}
      {onregister}
      {onresendverification}
      {onverifyemail}
      {onrequestpasswordreset}
      {onresetpassword}
    />

    <p class="account-note">Your account is shared across desktop and mobile readers.</p>
  </section>
</main>

<style>
  .login-screen {
    display: grid;
    min-height: 100dvh;
    place-items: center;
    padding: max(1.25rem, env(safe-area-inset-top))
      max(1.25rem, env(safe-area-inset-right)) max(1.25rem, env(safe-area-inset-bottom))
      max(1.25rem, env(safe-area-inset-left));
    color: var(--color-ink-text);
    background: var(--color-surface-bg);
    overflow-y: auto;
  }
  .login-card {
    width: min(100%, 27rem);
    padding: clamp(1.5rem, 5vw, 2.5rem);
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: color-mix(in srgb, var(--color-raised-bg) 96%, transparent);
    box-shadow: 0 1.5rem 4rem var(--color-box-shadow);
  }
  .heading {
    margin-bottom: 1.55rem;
  }
  .heading p {
    margin: 0 0 0.45rem;
    color: var(--color-accent);
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  h1 {
    margin: 0;
    font: 600 clamp(1.7rem, 8vw, 2.4rem)/1.1 var(--font-interface-family);
    letter-spacing: -0.02em;
  }
  .heading span {
    display: block;
    margin-top: 0.6rem;
    color: var(--color-subtle-text);
    font-size: 0.82rem;
    line-height: 1.55;
  }
  .account-note {
    margin: 1.15rem 0 0;
    padding-top: 1rem;
    border-top: 1px solid var(--color-border);
    color: var(--color-subtle-text);
    font-size: 0.68rem;
    line-height: 1.5;
  }
</style>
