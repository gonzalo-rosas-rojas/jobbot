() => {
  // Lists the controls of the application page for the filler and tags each one with
  // `data-jb="<idx>"` so the Rust side can address it again. One entry per control:
  //   idx, tag, type (kind), label, option, name, required, checked   (see `FieldInfo` in form.rs)
  //
  // What is reported, from what the real Greenhouse and Ashby forms do:
  // - Text, textarea, select and file controls get their own label. For a file input that is the
  //   caption of the `role=group` around it (Greenhouse's own label for it is a hidden "Attach"),
  //   and a control with no usable `label` takes the label of the field entry that holds only it
  //   (Ashby's typeahead and date picker have no id, so their `label[for]` points at nothing).
  //   The placeholder and the `name` are the last resort.
  // - kind: the `type` attribute, except `combobox` (react-select, Ashby's typeahead: `role=combobox`)
  //   and `date` (date pickers drawn as text inputs).
  // - Radio buttons, checkbox groups and toggle-button groups (Ashby's Yes/No: two buttons with
  //   `aria-pressed` and a hidden checkbox) are one question each: every option is an entry whose
  //   `label` is the question, `option` its own text and `name` the group key, so the planner sees
  //   one question with several options. A checkbox that stands alone keeps its own text as its label;
  //   one alone in a group with a caption is labelled "caption: option". `checked` tells which options
  //   are selected right now (a pressed toggle button, a checked radio or checkbox).
  // - Not reported: react-select's invisible helper input (`aria-hidden`, or the `...-requiredInput`
  //   input beside a combobox), hidden inputs, and the "autofill from resume" upload (it parses the
  //   CV and rewrites the form).
  // - Not reported either: honeypots. Boards catch bots with text boxes named like the real ones
  //   (`email`, `phone`, `name`) that a person can never see or reach, and a form that arrives with
  //   one filled is dropped without a word. A text box counts only when it is clearly visible: laid
  //   out, not `visibility: hidden`, not opacity 0 (nor inside an ancestor that is), at least 2px
  //   wide and tall, not pushed off the top or left of the page, not clipped away by a collapsed
  //   parent, not inside `aria-hidden`. A text box that is out of the tab order (`tabindex="-1"`)
  //   and has no real label (a label, `aria-label(ledby)` or a caption; a placeholder or a name is
  //   not one) is left out too. Checkboxes, radios, selects, comboboxes and file inputs are drawn
  //   with an invisible native element behind a styled one by design, so they keep the plain
  //   `offsetParent` test. A name is never used as the label of an input that is not clearly
  //   visible.
  // - required: the `required` attribute, `aria-required`, a star at the end of the label text, or a
  //   star the page draws with CSS (`::after`) or marks with a `required` class on the label.
  document.querySelectorAll('[data-jb]').forEach((el) => el.removeAttribute('data-jb'));
  const clean = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const textOf = (node) => clean(node.innerText || node.textContent);
  const typeOf = (el) => (el.getAttribute('type') || '').toLowerCase();
  const skipTypes = ['hidden', 'submit', 'button', 'image', 'reset'];
  const shown = (el) => el.offsetParent !== null;
  const tiny = (rect) => rect.width < 2 || rect.height < 2;
  // The element is laid out and a person could see and reach it (see the honeypots note above).
  const clearlyShown = (el) => {
    if (!shown(el) || el.closest('[aria-hidden="true"]')) return false;
    const rect = el.getBoundingClientRect();
    if (tiny(rect)) return false;
    // Pushed off the top or the left edge of the page, where no scrolling reaches it.
    if (rect.right + window.scrollX <= 0 || rect.bottom + window.scrollY <= 0) return false;
    if (['hidden', 'collapse'].includes(getComputedStyle(el).visibility)) return false;
    for (let node = el; node && node !== document.documentElement; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (parseFloat(style.opacity) < 0.05) return false;
      // A parent that clips its content and has no room for it.
      if (node !== el && style.overflow !== 'visible' && tiny(node.getBoundingClientRect())) return false;
    }
    return true;
  };
  // Text boxes are what a honeypot imitates.
  const isTextEntry = (el) =>
    el.tagName === 'TEXTAREA' ||
    (el.tagName === 'INPUT' && !['checkbox', 'radio', 'file'].includes(typeOf(el)) && el.getAttribute('role') !== 'combobox');
  // react-select (Greenhouse) adds an invisible `required` input after every drop-down for the
  // browser's own validation: a generated `...-requiredInput` class, no id, no name, no label, and
  // a combobox beside it that is the real control. Recognised by that shape and not by `aria-hidden`
  // alone, which a board may drop; an unlabeled required input with no combobox beside it is not one.
  const isSelectHelper = (el) =>
    el.tagName === 'INPUT' && !el.id && !el.getAttribute('name') &&
    /requiredinput/i.test(typeof el.className === 'string' ? el.className : '') &&
    Boolean(el.parentElement && el.parentElement.querySelector('[role="combobox"]'));

  let natives = Array.from(document.querySelectorAll('input, textarea, select')).filter((el) => {
    const type = typeOf(el);
    if (skipTypes.includes(type)) return false;
    if (type === 'file') return !el.closest('[class*="autofill" i]');
    if (el.getAttribute('aria-hidden') === 'true' || isSelectHelper(el)) return false;
    return isTextEntry(el) ? clearlyShown(el) : shown(el);
  });
  // Toggle buttons that sit together are the options of one question.
  const pressed = Array.from(document.querySelectorAll('button[aria-pressed]')).filter(shown);
  const buttonGroups = [];
  new Set(pressed.map((b) => b.parentElement)).forEach((parent) => {
    const buttons = pressed.filter((b) => b.parentElement === parent);
    if (buttons.length >= 2) buttonGroups.push({ parent, buttons });
  });

  const holdsOthers = (node, own) =>
    natives.some((n) => !own.includes(n) && node.contains(n)) ||
    buttonGroups.some((g) => !own.includes(g.parent) && node.contains(g.parent));
  // The first label or legend of the closest wrapper around `anchor` that holds no other control
  // and whose `for` does not point at some other element.
  const captionNear = (anchor, own) => {
    let node = anchor.parentElement;
    for (let depth = 0; node && node !== document.body && node.tagName !== 'FORM' && depth < 6; depth += 1) {
      if (holdsOthers(node, own)) return null;
      const caption = Array.from(node.querySelectorAll('label, legend')).find(
        (l) => !l.contains(anchor) && !l.control && textOf(l)
      );
      if (caption) return caption;
      node = node.parentElement;
    }
    return null;
  };

  const requiredClass = /(^|[\s_-])required($|[\s_-])/i;
  const optionalClass = /(not|non|un)[_-]?required/i;
  // Does this caption say the question is required?
  const marked = (node) => {
    if (!node) return false;
    if (/\*\s*$/.test(textOf(node))) return true;
    const classes = [node, ...node.querySelectorAll('*')].map((n) => (typeof n.className === 'string' ? n.className : ''));
    if (classes.some((c) => requiredClass.test(c) && !optionalClass.test(c))) return true;
    return ['::after', '::before'].some((pseudo) => {
      const content = getComputedStyle(node, pseudo).content;
      return Boolean(content) && content.includes('*');
    });
  };
  const idsText = (ids) =>
    clean((ids || '').split(/\s+/).map((id) => { const n = document.getElementById(id); return n ? n.innerText : ''; }).join(' '));

  // The control's own label: { text, node (the element the text came from), boxRequired, real }.
  // `real` is false only for the last resort, the placeholder or the name: those are no label.
  const labelOf = (el) => {
    if (typeOf(el) === 'file') {
      const group = el.closest('[role="group"][aria-labelledby]');
      const text = group ? idsText(group.getAttribute('aria-labelledby')) : '';
      if (text) {
        const first = group.getAttribute('aria-labelledby').split(/\s+/)[0];
        return { text, node: document.getElementById(first), boxRequired: group.getAttribute('aria-required') === 'true', real: true };
      }
    }
    let node = el.id ? document.querySelector('label[for="' + CSS.escape(el.id) + '"]') : null;
    if (!node || !textOf(node)) node = el.closest('label');
    if (node && textOf(node)) return { text: textOf(node), node, boxRequired: false, real: true };
    const by = el.getAttribute('aria-labelledby');
    if (idsText(by)) return { text: idsText(by), node: document.getElementById(by.split(/\s+/)[0]), boxRequired: false, real: true };
    const aria = clean(el.getAttribute('aria-label'));
    if (aria) return { text: aria, node: null, boxRequired: false, real: true };
    const near = captionNear(el, [el]);
    if (near) return { text: textOf(near), node: near, boxRequired: false, real: true };
    // The name says what a field is called in the page's code, not what it asks: a hidden input
    // named "email" must not look like the email question.
    const last = el.getAttribute('placeholder') || (clearlyShown(el) ? el.getAttribute('name') : '');
    return { text: clean(last), node: null, boxRequired: false, real: false };
  };
  // A text box out of the tab order with no real label is one no person reaches: only a bot
  // would fill it.
  natives = natives.filter(
    (el) => !(isTextEntry(el) && el.getAttribute('tabindex') === '-1' && !labelOf(el).real)
  );

  const isDatePicker = (el) =>
    /(^|[\s_-])date(picker)?($|[\s_-])/i.test(el.className || '') ||
    Boolean(el.closest('[class*="datepicker" i]')) ||
    /^(pick|select|choose)( a)? date|^(mm|dd|yyyy)[\/.-]/i.test(el.getAttribute('placeholder') || '');
  const kindOf = (el) => {
    const type = typeOf(el);
    if (el.tagName !== 'INPUT' || !['', 'text', 'search'].includes(type)) return type;
    if (el.getAttribute('role') === 'combobox') return 'combobox';
    return isDatePicker(el) ? 'date' : type;
  };

  // A radio or checkbox group: its members, the caption that asks the question, a key that
  // identifies it and whether it must be answered.
  const groupCache = new Map();
  const entryId = (node) => node.id || node.closest('[data-field-path]')?.getAttribute('data-field-path') || '';
  const captionIn = (box) => {
    const legend = box.querySelector('legend');
    if (legend && textOf(legend)) return { text: textOf(legend), node: legend };
    const by = box.getAttribute('aria-labelledby');
    if (idsText(by)) return { text: idsText(by), node: document.getElementById(by.split(/\s+/)[0]) };
    const aria = clean(box.getAttribute('aria-label'));
    if (aria) return { text: aria, node: null };
    const label = Array.from(box.querySelectorAll('label')).find((l) => !l.control && textOf(l));
    return label ? { text: textOf(label), node: label } : null;
  };
  // The text just above `start`, looking at the earlier siblings of it and of its ancestors up to
  // `boundary` and skipping the ones that hold a control (the other options).
  const textAbove = (start, boundary) => {
    for (let node = start; node && node !== boundary; node = node.parentElement) {
      for (let sib = node.previousElementSibling; sib; sib = sib.previousElementSibling) {
        const isControl = /^(INPUT|SELECT|TEXTAREA)$/.test(sib.tagName) || sib.querySelector('input, select, textarea');
        if (!isControl && textOf(sib)) return textOf(sib);
      }
    }
    return '';
  };
  // Without any markup for it, the question is the first line around the group that is not an
  // option; when the wrapper also holds other controls, the text just above the first option.
  const captionByLines = (el, members) => {
    const options = members.map((m) => labelOf(m).text);
    for (let node = el.parentElement; node && node !== document.body; node = node.parentElement) {
      if (!members.every((m) => node.contains(m))) continue;
      if (natives.some((n) => !members.includes(n) && node.contains(n))) {
        const above = textAbove(members[0].closest('label') || members[0], node);
        if (above) return { text: above, node: null };
      }
      const lines = (node.innerText || '').split('\n').map(clean).filter(Boolean);
      const question = lines.find((line) => !options.includes(line));
      if (question) return { text: question, node: null };
    }
    return null;
  };
  const groupOf = (el) => {
    if (groupCache.has(el)) return groupCache.get(el);
    const type = typeOf(el);
    const sameType = natives.filter((n) => typeOf(n) === type);
    const name = el.getAttribute('name') || '';
    let members = [el];
    let container = null;
    const box = el.closest('fieldset, [role="radiogroup"], [role="group"]');
    if (box && !natives.some((n) => box.contains(n) && typeOf(n) !== type)) {
      container = box;
      const inside = sameType.filter((n) => box.contains(n));
      const named = name ? inside.filter((n) => n.getAttribute('name') === name) : [];
      members = named.length >= 2 ? named : inside;
      // Several groups share this box: its caption belongs to none of them in particular.
      if (named.length >= 2 && inside.length > named.length) container = null;
    } else if (name) {
      const peers = sameType.filter((n) => n.getAttribute('name') === name);
      if (peers.length >= 2) members = peers;
    }
    let question = null;
    if (container) {
      question = captionIn(container);
      if (!question) {
        const near = captionNear(container, members);
        if (near) question = { text: textOf(near), node: near };
      }
    }
    if (!question && members.length >= 2) question = captionByLines(el, members);
    const names = new Set(members.map((m) => m.getAttribute('name') || ''));
    const key =
      names.size === 1 && !names.has('') ? [...names][0]
        : (container && entryId(container)) || 'group-' + groupCache.size;
    const required =
      Boolean(container && container.getAttribute('aria-required') === 'true') ||
      marked(question && question.node) ||
      members.some((m) => m.required || m.getAttribute('aria-required') === 'true');
    const group = { members, container, question, key, required };
    members.forEach((m) => groupCache.set(m, group));
    return group;
  };
  const withoutStar = (text) => text.replace(/\s*\*+\s*$/, '');

  const items = [];
  natives.forEach((el) => {
    const type = typeOf(el);
    const own = labelOf(el);
    if (type === 'radio' || type === 'checkbox') {
      const group = groupOf(el);
      const grouped = group.members.length >= 2 || (group.container && group.question);
      if (type === 'radio') {
        items.push({
          el, tag: 'input', kind: type, option: own.text,
          label: group.question ? group.question.text : own.text,
          name: grouped ? group.key : el.getAttribute('name') || '',
          required: group.required,
        });
      } else if (group.members.length >= 2) {
        items.push({
          el, tag: 'input', kind: type, option: own.text,
          label: group.question ? group.question.text : own.text,
          name: group.key, required: group.required,
        });
      } else if (group.container && group.question) {
        items.push({
          el, tag: 'input', kind: type, option: own.text,
          label: withoutStar(group.question.text) + ': ' + own.text,
          name: group.key, required: group.required,
        });
      } else {
        items.push({
          el, tag: 'input', kind: type, option: '', label: own.text,
          name: el.getAttribute('name') || '',
          required: el.required || el.getAttribute('aria-required') === 'true' || marked(own.node) || /\*\s*$/.test(own.text),
        });
      }
      return;
    }
    items.push({
      el, tag: el.tagName.toLowerCase(), kind: kindOf(el), option: '', label: own.text,
      name: el.getAttribute('name') || '',
      required:
        el.required || el.getAttribute('aria-required') === 'true' || own.boxRequired ||
        marked(own.node) || /\*\s*$/.test(own.text),
    });
  });
  buttonGroups.forEach(({ parent, buttons }) => {
    const caption = captionNear(parent, [parent]);
    if (!caption) return;
    const hidden = parent.querySelector('input');
    const name = (hidden && hidden.getAttribute('name')) || caption.getAttribute('for') || 'buttons-' + items.length;
    const required =
      marked(caption) || parent.getAttribute('aria-required') === 'true' || Boolean(hidden && hidden.required);
    buttons.forEach((button) => {
      items.push({ el: button, tag: 'button', kind: 'radio', option: textOf(button), label: textOf(caption), name, required });
    });
  });

  // Is this option selected right now: a pressed toggle button, a checked radio or checkbox.
  const selected = (item) =>
    item.tag === 'button'
      ? item.el.getAttribute('aria-pressed') === 'true'
      : (item.kind === 'radio' || item.kind === 'checkbox') && item.el.checked;

  items.sort((a, b) => (a.el.compareDocumentPosition(b.el) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
  return items.map((item, i) => {
    item.el.setAttribute('data-jb', String(i));
    return {
      idx: i, tag: item.tag, type: item.kind, label: item.label, option: item.option,
      name: item.name, required: Boolean(item.required), checked: Boolean(selected(item)),
    };
  });
}
