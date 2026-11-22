<script lang="ts">
  import { onMount } from "svelte";
  import { getCachedTokens, waitForApiReady } from "$lib/api/client";
  import { bootstrapFromCloud, syncSessionToLocal } from "$lib/api/auth";
  import { authStore } from "$lib/stores/auth.svelte";
  import { accessStore } from "$lib/stores/access.svelte";
  import { router } from "$lib/router/router.svelte";
  import { ContextMenuHost, Logo, StateView, TitleBar } from "$lib/components";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { applyAppearance, watchAppearance } from "$lib/theme";
  import { posStore } from "$lib/pos/store.svelte";
  import { isTauriRuntime, LOCAL_API_URL, probeHealth } from "$lib/api/localApi";
  import { startupFailure, type StartupFailure } from "$lib/api/startup";
  import { restartApp } from "$lib/api/backup";
  import { watchStartup } from "$lib/stores/startupWatch";

  import Login from "./routes/Login.svelte";
  import Register from "./routes/Register.svelte";
  import LocalLogin from "./routes/LocalLogin.svelte";
  import LocalRegister from "./routes/LocalRegister.svelte";
  import AccessSwitch from "./routes/AccessSwitch.svelte";
  import Shell from "./routes/Shell.svelte";
  import Pos from "./routes/pos/Pos.svelte";

  /** Set when the data service could not start: one page replaces every screen. */
  let failure = $state<StartupFailure | null>(null);

  onMount(async () => {
    if (isTauriRuntime()) {
      void watchStartup({
        healthy: () => probeHealth(LOCAL_API_URL),
        recorded: startupFailure,
        sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      }).then((f) => (failure = f));
    }
    watchAppearance();
    void settingsStore.load().then(() => applyAppearance()).catch(() => {});
    void posStore.load();
    try {
      await authStore.init();
    } catch (err) {
      console.error("[app] bootstrap failed:", err);
      authStore.markAnonymous();
    }
    // Local API boots in parallel; domain routes need it, not the login screen.
    void waitForApiReady()
      .then(async () => {
        if (!authStore.isAuthenticated) return;
        // A standalone installation has no cloud: nothing to hand over or import.
        if (authStore.isLocal) return;
        const cached = getCachedTokens();
        if (cached) await syncSessionToLocal(cached);
        void bootstrapFromCloud();
      })
      .catch((err) => console.warn("[app] local API:", err));
    const onAuthRoute = ["login", "register"].includes(router.current);
    if (authStore.isAuthenticated && (onAuthRoute || router.current === "")) {
      router.replace("dashboard");
    } else if (!authStore.isAuthenticated && !onAuthRoute) {
      router.replace("login");
    }
  });

  const view = $derived.by(() => {
    if (authStore.status === "initializing" || !posStore.loaded) return "loading";
    if (!authStore.isAuthenticated) {
      // The card shows one of four forms: Connexion or Inscription, online or local.
      const way = router.current === "register" ? "register" : "login";
      return accessStore.mode === "local" ? `local-${way}` : way;
    }
    if (posStore.active) return "pos";
    return "shell";
  });

  $effect(() => {
    if (typeof document === "undefined") return;
    document.documentElement.dataset.pos = view === "pos" ? "true" : "false";
  });

  // What this PC is (fresh, local, online) decides which side of the switch is on offer.
  const onAuthView = $derived(view !== "loading" && view !== "pos" && view !== "shell");
  $effect(() => {
    if (onAuthView) void accessStore.refresh();
  });

  // -----------------------------------------------------------------
  // Card morph: both Login and Register are always mounted (absolutely
  // stacked). bind:offsetWidth/Height (powered by ResizeObserver under
  // the hood) tracks each one's natural size; the card animates
  // width/height to the active form's measurements while the inactive
  // form fades out and the active one fades in.
  // -----------------------------------------------------------------

  // Card padding + border must match the CSS values below — kept in JS
  // so we can size the card correctly around the measured form content
  // (box-sizing: border-box, so we add both to the layer's intrinsic size).
  const CARD_PAD = 24; // var(--space-6)
  const CARD_BORDER = 1; // var(--border-w)
  const CARD_CHROME = CARD_PAD * 2 + CARD_BORDER * 2;

  const sizes = $state({
    login: { w: 0, h: 0 },
    register: { w: 0, h: 0 },
    "local-login": { w: 0, h: 0 },
    "local-register": { w: 0, h: 0 },
  });
  type Layer = keyof typeof sizes;

  const activeW = $derived(sizes[view as Layer]?.w ?? 0);
  const activeH = $derived(sizes[view as Layer]?.h ?? 0);
  const ready = $derived(Object.values(sizes).every((s) => s.w > 0 && s.h > 0));
</script>

<div class="app">
  {#if failure}
    <div class="app__auth">
      <div class="app__auth-tb">
        <TitleBar showSearch={false} transparent />
      </div>
      <div class="boot">
        <StateView
          kind="error"
          title={failure.title}
          body={failure.message}
          actionLabel="Relancer Motodesk"
          onAction={() => void restartApp()}
        />
      </div>
    </div>
  {:else if view === "pos"}
    <div class="app__pos">
      <Pos />
    </div>
  {:else if view === "shell"}
    <Shell />
  {:else}
    <div class="app__auth">
      <!-- Floating, transparent title bar so the auth scene can paint
           all the way up to the window edge underneath. -->
      <div class="app__auth-tb">
        <TitleBar showSearch={false} transparent />
      </div>

      {#if view === "loading"}
        <div class="boot">
          <StateView kind="loading" />
        </div>
      {:else}
        <div class="auth">
          <aside class="auth__brand" aria-hidden="true">
            <div class="auth__brand-inner">
              <div class="auth__brand-top">
                <span class="auth__logo"><Logo size={20} /></span>
                <span class="auth__brand-name">Motodesk</span>
              </div>
              <div class="auth__brand-body">
                <h2 class="auth__brand-title">Votre atelier,<br />en un seul écran.</h2>
                <p class="auth__brand-tag">Atelier · Ventes · Facturation · Clients</p>
              </div>
              <div class="auth__brand-foot">v0.1.0</div>
            </div>
          </aside>

          <section class="auth__pane">
            <div class="auth__stack">
              <div
                class="auth__card"
                class:is-ready={ready}
                style:width={ready ? `${activeW + CARD_CHROME}px` : "auto"}
                style:height={ready ? `${activeH + CARD_CHROME}px` : "auto"}
              >
                <div
                  class="auth__layer auth__layer--login"
                  class:is-active={view === "login"}
                  bind:offsetWidth={sizes.login.w}
                  bind:offsetHeight={sizes.login.h}
                >
                  <Login />
                </div>
                <div
                  class="auth__layer auth__layer--register"
                  class:is-active={view === "register"}
                  bind:offsetWidth={sizes.register.w}
                  bind:offsetHeight={sizes.register.h}
                >
                  <Register />
                </div>
                <div
                  class="auth__layer auth__layer--login auth__layer--local"
                  class:is-active={view === "local-login"}
                  bind:offsetWidth={sizes["local-login"].w}
                  bind:offsetHeight={sizes["local-login"].h}
                >
                  <LocalLogin />
                </div>
                <div
                  class="auth__layer auth__layer--register auth__layer--local"
                  class:is-active={view === "local-register"}
                  bind:offsetWidth={sizes["local-register"].w}
                  bind:offsetHeight={sizes["local-register"].h}
                >
                  <LocalRegister />
                </div>
              </div>

              <AccessSwitch />
            </div>
          </section>
        </div>
      {/if}
    </div>
  {/if}
  <ContextMenuHost />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: transparent;
  }
  .app__pos {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .app__auth {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .app__auth-tb {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: var(--z-overlay, 50);
  }

  /* ----- Auth layout ----- */
  .auth {
    display: grid;
    grid-template-columns: 360px 1fr;
    flex: 1 1 auto;
    min-height: 0;
    background: var(--color-bg-app);
  }

  .auth__brand {
    background: var(--color-accent);
    color: var(--color-text-on-accent);
    min-width: 0;
    overflow: hidden;
  }
  .auth__brand-inner {
    height: 100%;
    padding: calc(var(--titlebar-h) + var(--space-4)) var(--space-6) var(--space-6);
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }
  .auth__brand-top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .auth__logo {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    /* The mark's own accent half would vanish on the accent panel: it sits on a tile. */
    background: var(--color-bg-surface);
  }
  .auth__brand-name {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    line-height: 1;
  }
  .auth__brand-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .auth__brand-title {
    font-size: var(--fs-brand);
    font-weight: var(--fw-semibold);
    line-height: 1.15;
    letter-spacing: var(--tracking-tight);
    color: var(--color-text-on-accent);
  }
  .auth__brand-tag {
    font-size: var(--fs-sm);
    color: var(--color-brand-fg-muted);
  }
  .auth__brand-foot {
    font-size: var(--fs-xs);
    color: var(--color-brand-fg-subtle);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .auth__pane {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--titlebar-h) + var(--space-6)) var(--space-6) var(--space-6);
    background: var(--color-bg-app);
    min-width: 0;
    overflow: auto;
  }

  .auth__stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
  }

  /* ----- Morphing card ----- */
  .auth__card {
    position: relative;
    background: var(--color-bg-surface);
    border: var(--border-w) solid var(--color-border);
    padding: var(--space-6);
    box-shadow: var(--shadow-sm);
    box-sizing: border-box;
    overflow: hidden;
    border-radius: var(--radius-card);
    /* No transition until both forms have been measured to avoid the
       card animating in from auto/0 on first paint. */
  }
  .auth__card.is-ready {
    transition:
      width 360ms cubic-bezier(0.2, 0, 0, 1),
      height 360ms cubic-bezier(0.2, 0, 0, 1);
  }

  /* Each form is centred inside the card so it stays put visually while
     the card morphs around it. Each layer is "anchored" to its own
     side: login → left, register → right. When inactive it sits offset
     to that side and is invisible; when active it slides to centre and
     fades in. Together this gives the wizard-style direction in both
     transitions automatically. */
  .auth__layer {
    position: absolute;
    top: 50%;
    left: 50%;
    opacity: 0;
    pointer-events: none;
    will-change: transform, opacity;
    transition:
      opacity 180ms ease,
      transform 280ms cubic-bezier(0.2, 0, 0, 1);
  }
  .auth__layer--login {
    transform: translate(calc(-50% - 32px), -50%);
  }
  .auth__layer--register {
    transform: translate(calc(-50% + 32px), -50%);
  }
  /* The local forms sit a little lower: flipping the switch slides the card's content
     up or down, the way Connexion and Inscription slide sideways. */
  .auth__layer--login.auth__layer--local {
    transform: translate(calc(-50% - 32px), calc(-50% + 24px));
  }
  .auth__layer--register.auth__layer--local {
    transform: translate(calc(-50% + 32px), calc(-50% + 24px));
  }
  .auth__layer.is-active {
    opacity: 1;
    pointer-events: auto;
    transform: translate(-50%, -50%);
    /* Delay the fade-in slightly so the size morph is already underway
       when the new form starts appearing. */
    transition:
      opacity 240ms ease 120ms,
      transform 320ms cubic-bezier(0.2, 0, 0, 1) 60ms;
  }

  .boot {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    flex: 1 1 auto;
    background: var(--color-bg-app);
    color: var(--color-text-muted);
  }
  @media (max-width: 720px) {
    .auth {
      grid-template-columns: 1fr;
    }
    .auth__brand-inner {
      padding: calc(var(--titlebar-h) + var(--space-3)) var(--space-4) var(--space-4);
    }
    .auth__brand-body {
      display: none;
    }
  }
</style>
