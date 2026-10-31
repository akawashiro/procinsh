// Show privacy-mode pseudonyms as blurred blocks wherever pages put text.
// The server never sends the original text: a pseudonym is `░` + 8 hex
// digits + `░`, so blurring only marks what was hidden.

const PSEUDONYM = /░[0-9a-f]{8}░/g;
const STYLE = `.masked{filter:blur(3px);user-select:none;border-radius:3px;background:rgba(147,162,185,.3)}`;

/** Wraps each pseudonym in `text` in a blurred `<span class="masked">`. */
export function maskTextNode(text: Text): void {
  const content = text.data;
  if (!content.includes("░") || text.parentElement?.closest(".masked")) return;
  PSEUDONYM.lastIndex = 0;
  const fragment = document.createDocumentFragment();
  let end = 0;
  for (const match of content.matchAll(PSEUDONYM)) {
    fragment.append(content.slice(end, match.index));
    const span = document.createElement("span");
    span.className = "masked";
    span.title = "Hidden by privacy mode";
    span.textContent = match[0];
    fragment.append(span);
    end = match.index + match[0].length;
  }
  if (end === 0) return;
  fragment.append(content.slice(end));
  text.replaceWith(fragment);
}

function maskTree(node: Node): void {
  if (node instanceof Text) return maskTextNode(node);
  const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  while (walker.nextNode()) texts.push(walker.currentNode as Text);
  texts.forEach(maskTextNode);
}

/** Blurs pseudonyms already in `root` and any added later. */
export function watchMasked(root: Node = document.body): MutationObserver {
  const style = document.createElement("style");
  style.textContent = STYLE;
  document.head.append(style);
  maskTree(root);
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === "characterData") maskTree(record.target);
      else record.addedNodes.forEach(maskTree);
    }
  });
  observer.observe(root, {
    childList: true,
    subtree: true,
    characterData: true,
  });
  return observer;
}
