<script lang="ts">
  import { ArrowRight, Eye, EyeOff, LoaderCircle } from "lucide-svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { accountLink, localDestination } from "$lib/access/navigation";
  import { api } from "$lib/api";
  import { catalogQuery } from "$lib/catalog/query.svelte";
  import { t } from "$lib/i18n/index.svelte";
  import { getAuthStore } from "$lib/stores/auth.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import LoginPresentation from "./LoginPresentation.svelte";

  let { mode }: { mode: "login" | "register" } = $props();
  const auth = getAuthStore();
  const instance = catalogQuery(() => api.get<{ name: string; description: string; open_registration: boolean }>("/nodeinfo"));
  const fieldStyle = "h-10 bg-background focus-visible:ring-2";
  const submitStyle = "h-10 w-full gap-2";
  const passwordId = $props.id();

  let email = $state("");
  let username = $state("");
  let password = $state("");
  let confirmation = $state("");
  let revealPassword = $state(false);
  let busy = $state(false);
  let error = $state("");
  const destination = $derived(localDestination(page.url.searchParams.get("next"), page.url.origin));
  const register = $derived(mode === "register");
  const closed = $derived(register && instance.data?.open_registration === false);
  const title = $derived(closed ? t("auth.registrationClosed") : register ? t("auth.register") : t("auth.loginHeading"));

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || (register && !instance.data?.open_registration)) return;
    error = "";
    if (register && confirmation !== password) {
      error = t("setup.passwordMismatch");
      return;
    }
    busy = true;
    try {
      if (register) await auth.register(email, username, password);
      else await auth.login(username, password);
      await goto(destination);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : t("common.error");
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>{title} — SoundTime</title></svelte:head>

{#snippet accountForm()}
  {#if register && instance.loading}
    <p role="status" class="text-sm text-muted-foreground">{t("common.loading")}</p>
  {:else if register && typeof instance.data?.open_registration !== "boolean"}
    <p role="status" class="mb-4 text-sm text-muted-foreground">{t("access.registrationUnavailable")}</p>
    <Button variant="outline" onclick={instance.retry}>{t("access.retry")}</Button>
  {:else if closed}
    <p class="mb-5 text-sm text-muted-foreground">{t("auth.registrationClosedHint")}</p>
    <Button href={accountLink("/login", destination)} class={submitStyle}>{t("auth.signIn")}<ArrowRight size={15} aria-hidden="true" /></Button>
  {:else}
    <form onsubmit={submit} aria-busy={busy}>
      {#if register}
        <label class="field">
          <span>{t("auth.email")}</span>
          <Input type="email" name="email" autocomplete="email" class={fieldStyle} placeholder={t("auth.emailExample")}
            bind:value={email} required />
        </label>
      {/if}

      <label class="field">
        <span>{t("auth.username")}</span>
        <Input type="text" name="username" autocomplete="username" autocapitalize="none" spellcheck={false}
          placeholder={t("auth.usernameExample")} class={fieldStyle} bind:value={username} required />
      </label>

      <div class="field">
        <label for={passwordId}>{t("auth.password")}</label>
        <div class="relative">
          <Input type={revealPassword ? "text" : "password"} id={passwordId} name="password" placeholder="••••••••"
            autocomplete={register ? "new-password" : "current-password"} minlength={register ? 8 : undefined}
            class={fieldStyle + " pr-11"} bind:value={password} required />
          <button type="button" class="password-toggle" onclick={() => revealPassword = !revealPassword}
            aria-label={revealPassword ? t("auth.hidePassword") : t("auth.showPassword")} aria-controls={passwordId}>
            {#if revealPassword}<EyeOff size={16} />{:else}<Eye size={16} />{/if}
          </button>
        </div>
      </div>
      {#if register}
        <label class="field">
          <span>{t("auth.confirmPassword")}</span>
          <Input type="password" name="confirm" autocomplete="new-password" placeholder="••••••••"
            class={fieldStyle} bind:value={confirmation} required />
        </label>
      {/if}
      {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
      <Button type="submit" class={submitStyle} disabled={busy}>
        {#if busy}
          <LoaderCircle size={15} class="motion-safe:animate-spin" aria-hidden="true" />
          {register ? t("auth.registering") : t("auth.loggingIn")}
        {:else}
          {register ? t("auth.register") : t("auth.signIn")}<ArrowRight size={15} aria-hidden="true" />
        {/if}
      </Button>
    </form>
    {#if register || instance.data?.open_registration}
      <p class="alternate">
        {register ? t("auth.hasAccount") : t("auth.noAccount")}
        <a href={accountLink(register ? "/login" : "/register", destination)}>{register ? t("auth.signIn") : t("auth.signUp")}</a>
      </p>
    {/if}
  {/if}
{/snippet}

<LoginPresentation {title} subtitle={closed ? undefined : t(register ? "auth.registerSubtitle" : "auth.loginSubtitle")}
  instanceName={instance.data?.name} instanceDescription={instance.data?.description}>
  {@render accountForm()}
</LoginPresentation>

<style>
  form { display: grid; gap: 17px; }
  .field { display: grid; gap: 6px; color: hsl(var(--foreground)); font-size: 14px; font-weight: 500; }
  .field :global(input) { font-weight: 400; }
  .password-toggle {
    position: absolute; inset: 0 0 0 auto; display: grid; place-items: center;
    width: 40px; border-radius: 7px; color: hsl(var(--muted-foreground));
  }
  .password-toggle:hover { color: hsl(var(--foreground)); }
  .password-toggle:focus-visible { outline-offset: -3px; }
  .alternate { margin-top: 26px; color: hsl(var(--muted-foreground)); font-size: 14px; line-height: 1.5; }
  .alternate a { color: hsl(var(--ring)); font-weight: 550; text-underline-offset: 3px; }
  .alternate a:hover, .alternate a:focus-visible { text-decoration: underline; }
</style>
