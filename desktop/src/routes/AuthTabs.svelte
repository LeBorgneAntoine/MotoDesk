<script lang="ts">
  /**
   * The two things to do at the door: sign in, or register. The switch under the card says
   * whether that is with an online account or a local one. Shared by the four forms so the
   * card keeps one look.
   */
  import { Tabs, type TabItem } from "$lib/components";
  import { router } from "$lib/router/router.svelte";

  type Mode = "login" | "register";

  interface Props {
    active: Mode;
  }

  let { active }: Props = $props();

  const tabs: TabItem<Mode>[] = [
    { id: "login", label: "Connexion" },
    { id: "register", label: "Inscription" },
  ];
</script>

<Tabs {tabs} {active} size="sm" ariaLabel="Connexion ou inscription" onChange={(id) => router.replace(id)} />
