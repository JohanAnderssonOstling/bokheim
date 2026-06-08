<svelte:head>
  <title>Hello World</title>
</svelte:head>

<main class="shell">
  <div class="card">
    <p class="eyebrow">Tauri + Svelte</p>
    <h1>Hello World</h1>
    <p class="subhead">The mobile UI is rendering on-device.</p>
  </div>
</main>

<style>
:root {
  font-family: "IBM Plex Sans", "Avenir Next", sans-serif;
  color: #f4f5ef;
  background:
    radial-gradient(circle at top, rgba(255, 210, 122, 0.25), transparent 30%),
    linear-gradient(180deg, #1f2833 0%, #10161c 100%);
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

main {
  min-height: 100vh;
}

.shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 1.25rem;
}

.card {
  width: min(32rem, 100%);
  padding: 2rem 1.5rem;
  border-radius: 1.5rem;
  text-align: center;
  background: rgba(17, 24, 31, 0.82);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 24px 80px rgba(0, 0, 0, 0.28);
}

.eyebrow {
  margin: 0 0 0.75rem;
  font-size: 0.8rem;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: #ffcf8a;
}

h1 {
  margin: 0;
  font-size: clamp(2.8rem, 12vw, 4.8rem);
  line-height: 0.95;
  letter-spacing: -0.06em;
}

.subhead {
  margin: 1rem 0 0;
  font-size: 1rem;
  color: #d6d9df;
}
</style>
