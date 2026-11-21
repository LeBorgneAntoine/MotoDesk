<script lang="ts">
  /**
   * The Motodesk mark: two linked bars. The lower one is the neutral ink colour, the
   * upper one follows the accent colour chosen in Réglages › Apparence, so the logo
   * always matches the rest of the app. (The installer icon cannot follow the setting;
   * it is drawn with the default accent: `src-tauri/icons/source.svg`.)
   */
  interface Props {
    /** Rendered height in px; the width follows the mark's proportions. */
    size?: number;
    label?: string;
  }

  let { size = 28, label = "Motodesk" }: Props = $props();
</script>

<svg
  class="logo"
  viewBox="-4 -2 210 131"
  height={size}
  width={(size * 210) / 131}
  role="img"
  aria-label={label}
>
  <path
    class="logo__ink"
    d="M39 43 H114.1 Q117.9 43 116.8 47 L110.1 69 C109.3 71.7 107.4 73 104.6 73 H76.7 C69.4 73 62.2 78.7 62.2 86.3 C62.2 92.8 67 96.8 74.4 96.8 H126.6 L123.5 106.8 C119.7 119.5 112.4 127 99.4 127 H19.4 C6.3 127 -2.6 116.3 0.9 103.2 L14.1 62 C17.6 50.2 26.3 43 39 43 Z"
  />
  <path
    class="logo__accent"
    d="M102.1 0 H181.3 C194.1 0 203.9 9.8 201.2 23.2 L188.8 71 C185.8 82.4 178.2 90 166 90 H131 L142.2 48 C144.7 38.2 139.4 31 129 31 H72.8 L76.3 18.4 C80 6.7 89.2 0 102.1 0 Z"
  />
</svg>

<style>
  .logo {
    display: block;
    flex-shrink: 0;
  }
  .logo__ink {
    fill: var(--color-logo-ink);
  }
  .logo__accent {
    fill: var(--color-accent);
  }
</style>
