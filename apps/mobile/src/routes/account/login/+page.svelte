<script lang="ts">
  import { goto } from "$app/navigation";
  import LoginScreen from "./LoginScreen.svelte";
  import { commands } from "../../../lib/app/commands";

  let busy = $state(false);
  let progress = $state("");
  let error = $state("");

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  async function run(progressLabel: string, request: () => Promise<unknown>) {
    if (busy) return false;
    busy = true;
    error = "";
    progress = progressLabel;
    try {
      await request();
      progress = "";
      return true;
    } catch (reason) {
      error = message(reason);
      progress = "";
      return false;
    } finally {
      busy = false;
    }
  }

  async function login(email: string, password: string) {
    return run("Signing in…", async () => {
      await commands.login(email, password);
      await goto("/library", { replaceState: true });
    });
  }

  const register = (email: string, password: string) =>
    run("Creating account…", () => commands.requestPublicRegistration(email, password));
  const resendVerification = (email: string) =>
    run("Sending verification PIN…", () => commands.resendVerification(email));
  const requestPasswordReset = (email: string) =>
    run("Requesting password reset…", () => commands.requestPasswordReset(email));
  const resetPassword = (token: string, password: string) =>
    run("Updating password…", () => commands.resetPassword(token, password));

  async function verifyEmail(email: string, pin: string) {
    return run("Verifying email…", async () => {
      await commands.verifyEmail(email, pin);
      await goto("/library", { replaceState: true });
    });
  }
</script>

<svelte:head>
  <title>Sign in · Bokheim</title>
</svelte:head>

<LoginScreen
  {busy}
  {progress}
  {error}
  onlogin={login}
  onregister={register}
  onresendverification={resendVerification}
  onverifyemail={verifyEmail}
  onrequestpasswordreset={requestPasswordReset}
  onresetpassword={resetPassword}
/>
