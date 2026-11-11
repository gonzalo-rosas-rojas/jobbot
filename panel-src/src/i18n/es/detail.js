// Spanish, the words of the drawer's content (ui/detail.js and ui/detail/*): the answer at the top, the lines under it and
// the form of "Registrar una respuesta". Warm and plain, "tú". The words that other places share with the drawer (the
// state names, the five layers, the actions, the headings `drawer.*`, `journey.*`) are in common.js. The English twin is
// ../en/detail.js: same keys, same {variables}.
export default {
  // ---- the answer: where this application is and what comes now (one sentence, then one line under it)
  "detail.now.aria": "Dónde está esta postulación y qué sigue",
  "detail.now.approve": "Esperando tu aprobación",
  "detail.now.robot.scored": "jobbot va a preparar el CV",
  "detail.now.robot.dryrun": "jobbot está llenando el formulario",
  "detail.now.robot.approved": "Aprobada: sale en la próxima ventana",
  "detail.now.robot.scored.sub": "En su próxima ventana; no tienes que hacer nada.",
  "detail.now.robot.dryrun.sub": "Es una prueba: todavía no se envía nada.",
  "detail.now.robot.approved.sub": "jobbot la envía solo; no tienes que hacer nada.",
  "detail.now.mine.replied": "Respondieron: te toca contestar",
  "detail.now.mine.technical_test": "Prueba técnica: la empresa espera tu entrega",
  "detail.now.mine.interview": "Entrevista: te toca prepararte",
  "detail.now.mine.offer": "Tienes una oferta por responder",
  "detail.now.mine.other": "Tienes un pendiente en esta postulación",
  "detail.now.check.replied": "Respondieron: define el siguiente paso",
  "detail.now.check.technical_test": "Prueba técnica sin tarea: anota cuándo la entregas",
  "detail.now.check.interview": "Entrevista sin tarea: anota cómo te preparas",
  "detail.now.check.ghosted": "Sin respuesta: decide si insistes",
  "detail.now.check.other": "Define el siguiente paso",
  "detail.now.check.sub": "Nadie tiene una tarea aquí: anota la siguiente o registra lo que pasó.",
  "detail.now.wait": "Esperando la respuesta de la empresa",
  "detail.now.stale.one": "Sin noticias hace {n} día",
  "detail.now.stale.other": "Sin noticias hace {n} días",
  "detail.now.stale.sub": "Decide si insistes o la cierras.",
  "detail.now.accepted": "Aceptada: este proceso terminó bien",
  "detail.now.rejected": "La empresa dijo que no: este proceso terminó",
  "detail.now.withdrawn": "La retiraste: este proceso terminó",
  "detail.now.state": "Estado: {state}",
  "detail.now.last": "Última novedad {when}.",
  "detail.actions.aria": "Qué hacer con esta postulación",

  // ---- the header
  "detail.salary.min": "Sueldo publicado: desde {amount} al mes",
  "detail.salary.max": "Sueldo publicado: hasta {amount} al mes",
  "detail.stage.aria": "Etapa: {name}",

  // ---- tasks, mails
  "detail.tasks.other": "Otras tareas de esta postulación",
  "detail.mail.from": "de {from}",

  // ---- the dry-run screenshot
  "detail.shot.failed": "No se pudo cargar la captura.",
  "detail.shot.zoom": "Ampliar",
  "detail.shot.fit": "Ajustar al ancho",

  // ---- "Registrar una respuesta"
  "detail.mark.note": "Nota (opcional)",
  "detail.mark.hint": "Elige lo que pasó. Nada cambia hasta que lo confirmes.",
  "detail.mark.change": "{company} pasa de «{from}» a «{to}».",
  "detail.mark.change.closes": "{company} pasa de «{from}» a «{to}» y queda cerrada.",
  "detail.mark.yes": "Sí, registrarlo",
  "detail.mark.no": "No, volver",
  "detail.mark.done": "Registrado: {company} pasó de «{from}» a «{to}».",
  "detail.mark.undo_until": "Si fue un error, puedes deshacerlo hasta las {hora}.",
  "detail.mark.undo": "Deshacer",
  "detail.mark.again": "Registrar otra respuesta",
  "detail.mark.last": "Último cambio: de «{from}» a «{to}». Puedes deshacerlo hasta las {hora}.",
  "detail.mark.it": "La postulación",

  // ---- how the score was worked out: the factors the server names today (any other name is shown as it comes)
  "detail.factor.fit": "Encaje con tu perfil",
  "detail.factor.compensation": "Sueldo",
  "detail.factor.stability": "Estabilidad",
  "detail.factor.learning": "Aprendizaje",
  "detail.factor.interview_style": "Estilo de entrevista",
  "detail.factor.other": "Otras señales",
  "detail.score.total": "Total",
  "detail.gaps.go": "Repasar en Aprender",
};
