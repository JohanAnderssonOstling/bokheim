<script lang="ts">
  import { APP_NAME } from "../../lib/application-name";
  import AccountAccessForms from "./AccountAccessForms.svelte";

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
  <section class="login-card" aria-labelledby="login-title">
    <div class="brand" aria-hidden="true">B</div>
    <div class="heading">
      <p>{APP_NAME}</p>
      <h1 id="login-title">Your library, everywhere</h1>
      <span>Sign in or create an account to open your synchronized library.</span>
    </div>

    <AccountAccessForms {busy} {progress} {error} {onlogin} {onregister} {onresendverification} {onverifyemail} {onrequestpasswordreset} {onresetpassword} />

    <p class="account-note">Your Bokheim account is also used for synchronization. No second login is required.</p>
  </section>
</main>

<style>
  .login-screen {
    display: grid;
    min-height: 100dvh;
    place-items: center;
    padding: max(1.25rem, env(safe-area-inset-top)) max(1.25rem, env(safe-area-inset-right)) max(1.25rem, env(safe-area-inset-bottom)) max(1.25rem, env(safe-area-inset-left));
    color: var(--color-ink-text);
    background:
      radial-gradient(circle at 15% 12%, color-mix(in srgb, var(--color-accent) 18%, transparent), transparent 24rem),
      radial-gradient(circle at 88% 88%, color-mix(in srgb, var(--color-accent) 10%, transparent), transparent 28rem),
      var(--color-surface-bg);
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
  .brand {
    display: grid;
    width: 3rem;
    height: 3rem;
    margin-bottom: 1.5rem;
    place-items: center;
    border-radius: 0;
    color: var(--color-raised-bg);
    background: var(--color-accent);
    font: 600 1.45rem var(--font-interface-family);
  }
  .heading { margin-bottom: 1.55rem; }
  .heading p { margin: 0 0 .4rem; color: var(--color-accent); font-size: .68rem; font-weight: 800; letter-spacing: .14em; text-transform: uppercase; }
  h1 { margin: 0; font: 500 clamp(2rem, 8vw, 2.7rem)/1.05 var(--font-interface-family); letter-spacing: -.035em; }
  .heading span { display: block; margin-top: .65rem; color: var(--color-subtle-text); font-size: .82rem; line-height: 1.55; }
  .account-note { margin: 1.15rem 0 0; padding-top: 1rem; border-top: 1px solid var(--color-border); color: var(--color-subtle-text); font-size: .68rem; line-height: 1.5; }
</style>
