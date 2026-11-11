// English, the words of the drawer's content (ui/detail.js and ui/detail/*): the answer at the top, the lines under it and
// the form of "Record an answer". Short and idiomatic, not a calque of the Spanish. The words that other places share with
// the drawer (the state names, the five layers, the actions, the headings `drawer.*`, `journey.*`) are in common.js.
// Same keys and {variables} as ../es/detail.js.
export default {
  // ---- the answer: where this application is and what comes now (one sentence, then one line under it)
  "detail.now.aria": "Where this application stands and what is next",
  "detail.now.approve": "Waiting for your approval",
  "detail.now.robot.scored": "jobbot will prepare the CV",
  "detail.now.robot.dryrun": "jobbot is filling in the form",
  "detail.now.robot.approved": "Approved: it goes out in the next window",
  "detail.now.robot.scored.sub": "In its next window; there is nothing for you to do.",
  "detail.now.robot.dryrun.sub": "This is a practice run: nothing is sent yet.",
  "detail.now.robot.approved.sub": "jobbot sends it on its own; there is nothing for you to do.",
  "detail.now.mine.replied": "They replied: your turn to answer",
  "detail.now.mine.technical_test": "Technical test: the company is waiting for your delivery",
  "detail.now.mine.interview": "Interview: time to get ready",
  "detail.now.mine.offer": "You have an offer to answer",
  "detail.now.mine.other": "You have something to do on this application",
  "detail.now.check.replied": "They replied: set the next step",
  "detail.now.check.technical_test": "Technical test with no task: note when you will deliver it",
  "detail.now.check.interview": "Interview with no task: note how you will prepare",
  "detail.now.check.ghosted": "No reply: decide whether to follow up",
  "detail.now.check.other": "Set the next step",
  "detail.now.check.sub": "Nobody has a task here: add the next one or record what happened.",
  "detail.now.wait": "Waiting for the company to reply",
  "detail.now.stale.one": "No news for {n} day",
  "detail.now.stale.other": "No news for {n} days",
  "detail.now.stale.sub": "Decide whether to follow up or close it.",
  "detail.now.accepted": "Accepted: this process ended well",
  "detail.now.rejected": "The company said no: this process is over",
  "detail.now.withdrawn": "You withdrew it: this process is over",
  "detail.now.state": "State: {state}",
  "detail.now.last": "Last news {when}.",
  "detail.actions.aria": "What to do with this application",

  // ---- the header
  "detail.salary.min": "Published salary: from {amount} a month",
  "detail.salary.max": "Published salary: up to {amount} a month",
  "detail.stage.aria": "Stage: {name}",

  // ---- tasks, mails
  "detail.tasks.other": "Other tasks for this application",
  "detail.mail.from": "from {from}",

  // ---- the capture of the practice run
  "detail.shot.failed": "The capture could not be loaded.",
  "detail.shot.zoom": "Enlarge",
  "detail.shot.fit": "Fit to width",

  // ---- "Record an answer"
  "detail.mark.note": "Note (optional)",
  "detail.mark.hint": "Pick what happened. Nothing changes until you confirm.",
  "detail.mark.change": "{company} moves from “{from}” to “{to}”.",
  "detail.mark.change.closes": "{company} moves from “{from}” to “{to}” and closes.",
  "detail.mark.yes": "Yes, record it",
  "detail.mark.no": "No, go back",
  "detail.mark.done": "Recorded: {company} moved from “{from}” to “{to}”.",
  "detail.mark.undo_until": "If it was a mistake, you can undo it until {hora}.",
  "detail.mark.undo": "Undo",
  "detail.mark.again": "Record another answer",
  "detail.mark.last": "Last change: from “{from}” to “{to}”. You can undo it until {hora}.",
  "detail.mark.it": "The application",

  // ---- how the score was worked out: the factors the server names today (any other name is shown as it comes)
  "detail.factor.fit": "Fit with your profile",
  "detail.factor.compensation": "Pay",
  "detail.factor.stability": "Stability",
  "detail.factor.learning": "Learning",
  "detail.factor.interview_style": "Interview style",
  "detail.factor.other": "Other signals",
  "detail.score.total": "Total",
  "detail.gaps.go": "Review in Learn",
};
