<script lang="ts">
  import { onMount } from 'svelte';
  export let name: string = 'world';
  let count = $state(0);
  const css = String.raw`a { color: red; }`;
  function inc(by: number): void {
    count += by;
  }
</script>

<!-- A comment -->
<main class="app" class:active={count > 0}>
  <h1 title="Hello {name}">Hello {name}!</h1>
  {#if count > 10}
    <p>Big: {count * 2}</p>
  {:else if count > 0}
    <p>Small</p>
  {:else}
    <p>None &amp; zero</p>
  {/if}
  {#each [1, 2, 3] as item, i (item)}
    <button on:click={() => inc(item)}>{i}: {item}</button>
  {/each}
  <Counter bind:value={count} {name} />
</main>

<style>
  main { padding: 1em; }
  .app > h1:hover { color: #ff3e00; }
</style>
