# Known gaps: the never-filled gate and the region rules

Two word-reading rules decide things nobody reviews one by one:

- `crates/jobbot-domain/src/never_filled.rs` (`is_never_filled`): the labels of form fields the
  program never fills, whatever the answer bank says (government ID numbers, tax numbers, banking).
- `crates/jobbot-domain/src/region.rs`: whether a posting's text closes the job to someone who
  works from Chile, is not a US citizen and holds no US clearance (time-zone residence and
  clearance sentences among others).

Both were reviewed adversarially for six rounds in November 2026. Each round closed holes and
opened smaller ones, so the work stopped at a bar instead of at zero: **a gap blocks a merge only
when a realistic label or sentence causes harm**. Harm means the gate lets an ID, tax or bank
number field be auto-filled, the region rule closes a job that is open, or a very common question
is sent to the person on most applications. Everything below was found, judged below that bar and
left on purpose. Fix one by adding its text to the tables in the tests first.

Which direction is safe:

- Gate: sending an ordinary question to the person costs attention; filling an ID field costs
  trust. When in doubt the gate does not fill.
- Region: a job wrongly left open is seen by the person before anything is sent; a job wrongly
  closed is lost without a trace. When in doubt the rule leaves the job open.

## Gate: labels that ask for a number and are still answerable

Whether any of these would really be answered depends on the answer bank having a pattern loose
enough to match; the gate just does not stop it.

- **"CI" in capitals with no number-word, hint or whole-label role**: "CI" is the ID card of
  Uruguay, Bolivia, Paraguay, Venezuela and Ecuador, but in forms for software people it is far
  more often continuous integration. It is read as the ID card only with a number-word beside it
  ("Número de CI", "Nro. CI", "CI Nro."), with a Spanish format hint ("CI (sin puntos)", "CI
  (solo números)") or as the whole label. "Cédula CI", "Documento: CI" or "CI del postulante" are
  not caught.
- **A format hint as the only number-word**: "Document (numbers only)", "Driver's license (numbers
  only)", "License (numbers only)", "Medicare (numbers only)", "Account (numbers only)", "Card
  (numbers only)". The hint is stripped so that "Years of experience (numbers only)" stays
  answerable, and what is left has no number-word. Real forms write "X number". A fix: when what
  is left holds a document word or a banking noun, read the hint as "number".
- **"Tax No (if any)"**, "Tax reference no (if any)": a final "No" after a bracketed remark is read
  as a number only after a document word, and "tax" is not one.
- **"Full name and ID (as on passport)"**: a weak "ID" with no number-word, in a name label.
- **Banking blocks without "account"**: "Bank information", "Banking information", "Bank info",
  "Payment details", "Payout details", "Wire transfer details", "Bank transfer details", "Direct
  deposit information", "Payout account details", "Account details (for payouts)".
- **Spanish account numbers in another order**: "N° cuenta", "Número cuenta", "Cuenta N°", "Cuenta
  (número)". Only "número de cuenta" and the English order are read.
- **"Run" in title case**: "Run (sin puntos ni guión)", "Ingrese su Run". Only "RUN" in capitals
  and "numero de run" count, because lowercase "run" is a verb.
- **Numbers of other countries and of immigration**: "Visa number", "Work permit number",
  "Residence permit number", "Alien registration number", "USCIS number", "I-94 number", CNPJ, RG,
  Steuer-ID, Personnummer, BSN, PESEL, NINO, "UAN number", UTR, "Unique Taxpayer Reference", "Tax
  reference", "Tax registration", CVU.
- **EIN, PAN and SIN alone**: answerable by design (ordinary words and acronyms in English); only
  "ein number", "pan card" and "sin number" are caught.
- **"C.I.N°"** with no space, and **"Full name, including tax-ID"** with a hyphen: unrealistic
  spellings.

## Gate: ordinary questions that go to the person

Harmless, and none is a common question:

- "C.I." meaning continuous integration with no word that says so: "Describe your C.I. setup",
  "What C.I. do you use?", "Your C.I. stack", "C.I. & C.D.", "Familiarity with C.I. / C.D." (with
  a space after the slash). The same words with "CI" (no dots) are answerable.
- "CI" alone as a whole label, or wrapped only in "your" or "please enter", in a form about
  continuous integration ("Your CI", "CI:"): read as the ID card, because nothing honest tells the
  two meanings apart.
- Yes/no questions about a document: "Do you have a tax ID?", "Do you have a (Chilean) RUT?".
- Name labels that cite two documents or two languages: "Full name (as on ID) / Nombre completo
  (según cédula)", "Full legal name (as on passport), must match your ID".
- "RFC writing experience" (mixed case, four words at most, reads as the Mexican tax ID).
- A number-word or an answer right after "C.I." meaning continuous integration: "Experience with
  C.I.? No", "C.I. N/A", "C.I. number of builds per day", "C.I. #1 tool you use". A number-word
  beside "C.I." always makes it the ID card.

## Region: sentences that close a job and arguably should not

- "Knowledge of DoD clearance requirements is a plus", "Familiarity with polygraph requirements is
  nice to have": "requirements" counts as a requirement word and blocks the wish.
- "The ideal candidate is based in North American time zones": "ideal" alone is not a wish
  ("ideally" is). The sentence is ambiguous.
- "Candidates based in US time zones will be prioritized": "prioritized" is not a wish word.
- Wishes spelled another way: "Nice-to-have: Active Secret clearance" (hyphens), "Plus: active
  Secret clearance", "Bonus if you hold an active Secret clearance", "Extra credit for an active
  Secret clearance", and a "Nice to have:" header on its own line (sentences are split at line
  breaks, so the header and its list are read apart).
- An adjective outside the closed list between the article and "plus", "bonus", "advantage" or
  "asset" ("a welcome plus"), or a wish said with a word the list does not have: the wish is not
  seen. This family has no end; each one found goes into `WISH_WORDS` with its sentence in the
  tests.
- A place that belongs to a party the candidate works with, said with a verb or a shape the rule
  does not know: "You must be comfortable working with clients in the US, most of whom are based
  in US time zones" ("whom", and words between the party and the verb). The verbs known today are
  with, alongside, support, serve, help, assist, "work for", "report to", "available to" and
  "responsive to".

## Region: sentences that leave a job open and should close it

The person still sees these before anything is sent.

- "Ideally you will hold an active TS/SCI clearance, which is required for this role": the wish
  before the clearance is taken out before the requirement is read.
- "Preferred: polygraph; active TS/SCI clearance": the whole sentence is read as a wish.
- A requirement on the candidate whose modal is more than six words before the verb, with another
  party in between: "You must work for our clients and be based in North American time zones".

## Limits of the method

- A clause is "about the candidate" by the six words before the verb. A modal further back is not
  seen, so "You should be comfortable working with a team based in..." and a longer rewording of
  the same sentence can differ.
- `read_label` runs again for every closing bracket and every peeled remark, which is quadratic in
  the label's length. Real labels are short; cap the length if hostile pages ever matter.

## Debt in the code, not in the behaviour

- The abbreviations of "number" live in four lists of `never_filled.rs` (`NUMBER_WORDS`,
  `BARE_WORD_LABELS`, `NUMBER_WORDS_BESIDE_AN_ID_TOKEN`, `TAX_ID_WORDS`), each with its own reason
  for holding or leaving out a plain "no". One source with those reasons written next to it would
  be safer.
- `TAX_ID_WORDS` and `TAX_QUALIFIERS` share six words and are asked together in `read_label`.
- The closure that decides what `TAX_BASIS` keeps belongs in a function of its own.
- The modal words ("must", "need to", "required to"...) are written in four places of `region.rs`
  with small differences (`CANDIDATE`, `CANDIDATE_SIDE`, the `must` of `CLEARANCE`,
  `REQUIREMENT_WORD`).
