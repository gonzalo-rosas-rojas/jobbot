# Example profile and CV

A made-up candidate's profile (`profile/*.toml`) and CV master (`cv/cv-master.md`). Every name,
company, address and number in them is fictional.

`cargo xtask export-public` copies these files to `profile/` and `cv/` in the public copy, so the
program and its tests run there without anyone's real data. To use jobbot yourself, copy them to
`profile/` and `cv/` (or to the folder `JOBBOT_HOME` names) and replace every value with your own.
