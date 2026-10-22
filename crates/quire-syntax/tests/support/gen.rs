//! Generated fixtures shared by tests and benches.

/// A Svelte component of about `lines` lines: a TypeScript script (~10%), markup with
/// expressions and blocks (~80%) and a style block (~10%).
#[allow(dead_code)]
pub fn svelte_component(lines: usize) -> String {
    let mut s = String::with_capacity(lines * 40);
    let mut push = |s: &mut String, chunk: &str| -> usize {
        s.push_str(chunk);
        chunk.matches('\n').count()
    };
    let mut count = push(&mut s, "<script lang=\"ts\">\n  import { onMount } from 'svelte';\n");
    let mut n = 0;
    while count < lines / 10 {
        count += push(&mut s, &format!(
            "  interface Item{n} {{\n    id: number;\n    name: string;\n  }}\n  let items{n}: Item{n}[] = $state([]);\n  function total{n}(xs: Item{n}[]): number {{\n    return xs.reduce((a, x) => a + x.id * {n}, 0);\n  }}\n"
        ));
        n += 1;
    }
    count += push(&mut s, "</script>\n\n<main class=\"app\">\n");
    let mut k = 0;
    while count < lines - lines / 10 {
        count += push(&mut s, &format!(
            "  <section id=\"s{k}\" class:active={{selected === {k}}}>\n    <h2 title=\"Section {k}\">{{title{k}}} &amp; more</h2>\n    {{#if items{m}.length > 0}}\n      <ul>\n        {{#each items{m} as item (item.id)}}\n          <li on:click={{() => pick(item.id)}}>{{item.name}}: {{total{m}(items{m})}}</li>\n        {{/each}}\n      </ul>\n    {{:else}}\n      <p>No items</p>\n    {{/if}}\n    <!-- section {k} -->\n  </section>\n",
            m = k % n.max(1)
        ));
        k += 1;
    }
    count += push(&mut s, "</main>\n\n<style>\n");
    let mut c = 0;
    while count < lines.saturating_sub(1) {
        count += push(&mut s, &format!(".s{c} {{\n  color: #{c:03x};\n  margin: {c}px 0;\n}}\n"));
        c += 1;
    }
    s.push_str("</style>\n");
    s
}
