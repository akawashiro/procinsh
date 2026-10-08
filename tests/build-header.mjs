import assert from 'node:assert/strict';

export async function checkBuildHeader(evaluate) {
  const build = await evaluate(`(() => {
    const brand = document.getElementById('brand'), commit = document.getElementById('build-commit');
    const bounds = commit?.getBoundingClientRect();
    return {brand: brand.textContent, brandHref: brand.getAttribute('href'),
      group: brand.parentElement.textContent, unresolved: document.querySelector('header').textContent.includes('{{PROCINSH_'),
      commit: commit && {text: commit.textContent, href: commit.href, title: commit.title,
        label: commit.getAttribute('aria-label'), target: commit.target, rel: commit.rel,
        visible: bounds.width>0 && bounds.left>=0 && bounds.right<=innerWidth && bounds.top>=0}};
  })()`);
  assert.match(build.brand, /^procinsh v\d+\.\d+\.\d+$/);
  assert.equal(build.brandHref, '/list', 'brand remains a list link');
  assert.equal(build.unresolved, false);
  if (!build.commit) {
    assert.equal(build.group, build.brand, 'version-only headers have no separator');
    return null;
  }
  const sha = build.commit.href.match(/^https:\/\/github.com\/akawashiro\/procinsh\/commit\/([0-9a-f]{40})$/)?.[1];
  assert.ok(sha, 'commit link uses a validated full SHA');
  assert.match(build.commit.text, new RegExp(`^${sha.slice(0,7)}(?:-dirty)?$`));
  assert.ok(build.commit.title.includes(sha));
  assert.ok(build.commit.label.includes(sha));
  assert.equal(build.commit.target, '_blank');
  assert.ok(build.commit.rel.includes('noopener') && build.commit.rel.includes('noreferrer'));
  assert.equal(build.commit.visible, true, 'commit remains visible at this viewport');
  return {sha, text: build.commit.text};
}
