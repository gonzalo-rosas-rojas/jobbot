// The structural rules of axe-core that the panel has broken, checked in Node over test/helpers/fake-dom.mjs (no browser, no layout): what
// a role needs around it. They are the same rules axe reports (aria-required-children, aria-required-parent, definition-list), restricted
// to the roles and tags the panel uses; the real axe run on the built page stays the judge, these keep a regression out of the sources.
//
//   problems(root)   [] when the subtree is sound, else one sentence per defect

const roleOf = (el) => (el.getAttribute("role") || "").trim().toLowerCase();
const NEUTRAL = new Set(["none", "presentation"]);

// role -> the roles its owned items must have / the roles its owner must have (ARIA 1.2, the ones this panel uses)
const NEEDS_CHILD = { menu: ["menuitem", "menuitemcheckbox", "menuitemradio", "group", "menu"], tablist: ["tab"], listbox: ["option", "group"] };
const NEEDS_PARENT = { menuitem: ["menu", "menubar", "group"], tab: ["tablist"], option: ["listbox", "group"] };

/** The element's owned children as the accessibility tree sees them: `none`/`presentation` boxes and role-less wrappers (ul, li, div) are looked through. */
function ownedItems(el) {
  const out = [];
  for (const kid of el.children) {
    const role = roleOf(kid);
    if (role && !NEUTRAL.has(role)) out.push(kid);
    else if (NEUTRAL.has(role) || /^(UL|OL|LI|DIV)$/.test(kid.tagName)) out.push(...ownedItems(kid));
  }
  return out;
}

/** The nearest ancestor that has a role of its own, looking through `none`/`presentation` and the list tags that hold an item. */
function ownerOf(el) {
  for (let up = el.parentElement; up; up = up.parentElement) {
    const role = roleOf(up);
    if (role && !NEUTRAL.has(role)) return up;
    if (!/^(UL|OL|LI)$/.test(up.tagName) && !NEUTRAL.has(role)) return null;
  }
  return null;
}

const label = (el) => `<${el.tagName.toLowerCase()}${el.className ? "." + el.className.trim().split(/\s+/).join(".") : ""}${roleOf(el) ? ` role=${roleOf(el)}` : ""}>`;

function walk(el, visit) {
  visit(el);
  for (const kid of el.children) walk(kid, visit);
}

export function problems(root) {
  const out = [];
  walk(root, (el) => {
    const role = roleOf(el);
    if (NEEDS_CHILD[role]) {
      const items = ownedItems(el);
      const stray = items.filter((i) => !NEEDS_CHILD[role].includes(roleOf(i)));
      if (items.length === 0) out.push(`aria-required-children: ${label(el)} owns no ${NEEDS_CHILD[role].join("/")}`);
      for (const s of stray) out.push(`aria-required-children: ${label(el)} owns ${label(s)}`);
      // text and paragraphs between the items (a title, a foot) are not owned items but axe reads them as content of the role
      for (const kid of el.children) if (!roleOf(kid) && /^(P|SPAN|H[1-6])$/.test(kid.tagName)) out.push(`aria-required-children: ${label(el)} holds ${label(kid)}, which is not one of its items`);
    }
    if (NEEDS_PARENT[role]) {
      const owner = ownerOf(el);
      if (!owner || !NEEDS_PARENT[role].includes(roleOf(owner))) out.push(`aria-required-parent: ${label(el)} sits in ${owner ? label(owner) : "nothing with a role"}, not in ${NEEDS_PARENT[role].join("/")}`);
    }
    if (el.tagName === "DL") {
      for (const kid of el.children) {
        if (kid.tagName === "DT" || kid.tagName === "DD") continue;
        if (kid.tagName !== "DIV") { out.push(`definition-list: ${label(el)} has ${label(kid)} as a direct child`); continue; }
        if (kid.hasAttribute("aria-hidden")) continue; // a skeleton row is hidden from the reader altogether
        for (const grand of kid.children) if (grand.tagName !== "DT" && grand.tagName !== "DD") out.push(`definition-list: ${label(kid)} in ${label(el)} has ${label(grand)}, not dt/dd`);
      }
    }
  });
  return out;
}
