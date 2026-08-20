<script lang="ts">
  let {
    value = $bindable(""),
    length = 6,
    disabled = false,
    ariaLabel = "Verification pin",
  }: {
    value?: string;
    length?: number;
    disabled?: boolean;
    ariaLabel?: string;
  } = $props();

  let focused = $state(false);

  function updateValue(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    value = input.value.replace(/\D/g, "").slice(0, length);
    input.value = value;
  }
</script>

<div
  class="pin-entry"
  class:focused
  style={`--pin-length: ${length}`}
>
  <input
    class="native-input"
    type="text"
    {value}
    maxlength={length}
    inputmode="numeric"
    autocomplete="one-time-code"
    pattern="[0-9]*"
    aria-label={ariaLabel}
    placeholder="Verification pin"
    {disabled}
    oninput={updateValue}
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
  />
  {#each Array.from({ length }) as _, index}
    <span
      class="pin-cell"
      class:active={focused && index === Math.min(value.length, length - 1)}
      aria-hidden="true">{value[index] ?? ""}</span
    >
  {/each}
</div>

<style>
  .pin-entry {
    position: relative;
    display: grid;
    width: 100%;
    min-width: 0;
    height: 2.65rem;
    grid-template-columns: repeat(var(--pin-length), minmax(0, 1fr));
    gap: 0.3rem;
  }
  .native-input {
    position: absolute;
    z-index: 2;
    inset: 0;
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    outline: 0;
    color: transparent;
    background: transparent;
    caret-color: transparent;
    -webkit-text-fill-color: transparent;
  }
  .native-input::placeholder {
    color: transparent;
  }
  .pin-cell {
    display: grid;
    min-width: 0;
    height: 2.65rem;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
    font-size: 0.9rem;
    font-weight: 750;
  }
  .pin-cell.active {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
  }
  .native-input:disabled ~ .pin-cell {
    opacity: 0.55;
  }
</style>
