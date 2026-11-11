# Color del panel: papel y tinta

El panel tiene dos temas que salen de las mismas reglas: **Papel** (claro, la mañana) y **Tinta** (oscuro, la noche
del turno). Siguen al sistema hasta que eliges con la tecla `T` o el botón «Tema»; la elección se guarda en este PC,
y `?theme=dark` o `?theme=light` en la dirección manda sobre todo (así lo usa la intro).

La idea: un programa cuya promesa es no inventar nada tiene que verse como lo que es. Honesto, en color, quiere decir
cuatro cosas:

1. **Un color, un significado.** Si el dorado señala «te toca», no puede decorar nada más.
2. **Proporción.** Lo atrasado se ve atrasado, pero no hay una pantalla entera de alarma.
3. **Se lee.** Nada de texto que haya que adivinar: todo pasa AA, el cuerpo pasa AAA.
4. **No depende del color.** Quien no distingue rojo de verde recibe lo mismo por otra vía.

## La teoría, aplicada

- **Ancla azul (tono 262 en OKLCH).** El azul es el color que más gente lee como confiable (la tinta de una firma, las
  instituciones, «true blue»). Es la tinta del texto y de toda la estructura: pestañas, línea del mapa, enlaces,
  selección. Los grises están teñidos del mismo azul (croma ≤ 0,03), así que nada desentona.
- **Complemento para decidir (ámbar, tono 85).** El ámbar es lo opuesto al azul en la rueda: el máximo contraste de
  tono. Por eso lo único cálido de la pantalla es lo que necesita a una persona. Regla 60-30-10: 60 % papel o tinta,
  30 % azul, 10 % o menos ámbar.
- **Verde solo para lo confirmado, rojo solo para lo atrasado.** Están a ambos lados del ámbar, y se usan en
  superficies pequeñas (una etiqueta, un sello, una barra), no como fondo de página.
- **Luminosidad antes que tono.** Los pares se diseñan por su luminosidad perceptual (OKLCH), no por su matiz, y
  recién después se les da color. Así el contraste está garantizado y sobrevive a la escala de grises.
- **Sin brillos ni difuminados.** No hay degradados de ambiente, ni resplandor, ni fondo borroso: una superficie plana
  dice lo que es. (El tablero conserva la sombra de sus hojas porque es un objeto mecánico.)
- **Orden con un solo matiz.** La nota del repaso (0 a 5) es una escala ordenada, así que va de gris a azul con el
  número escrito encima, no del rojo al verde.

## Los colores

| Token | Papel (claro) | Tinta (oscuro) | Significa |
| --- | --- | --- | --- |
| `--paper` / `--surface` / `--raise` | `#edf0f6` / `#f7f9fc` / `#ffffff` | `#0a101c` / `#121926` / `#1b2331` | la página, las tarjetas, lo que flota |
| `--ink` / `--muted` | `#131f35` / `#4c596c` | `#e9eff7` / `#9eacbe` | texto, texto secundario |
| `--line` / `--line-soft` | `#cbd2da` / `#dfe3ea` | `#2b3648` / `#1d2634` | estructura |
| `--track` (+ `-soft`, `-ink`) | `#2063b0` | `#61afee` | el camino: enlaces, línea, selección, «espera a la empresa» |
| `--gold` (+ `-soft`, `-text`, `-ink`) | `#fbc024` | `#fbc024` | **te toca** |
| `--ok` (+ `-soft`) | `#00563a` | `#5bf0b8` | confirmado, enviado |
| `--bad` (+ `-soft`) | `#c72d31` | `#ef6661` | vencido, falló |
| `--board`, `--chalk` | `#131f35`, `#edf2f9` | `#101a2d`, `#edf2f9` | el tablero: siempre oscuro |

Los valores salen de `scripts/palette.mjs` (OKLCH a sRGB, reduciendo solo el croma cuando no cabe en la gama).

## Qué se comprueba

`node scripts/palette.mjs` imprime, para los dos temas, el contraste de cada par que se lee y la separación entre los
cuatro significados bajo protanopía, deuteranopía y tritanopía (modelo de Machado, 2009; ΔE en OKLab).

- Cuerpo de texto: 14,5:1 en Papel y 16,4:1 en Tinta (AAA exige 7:1).
- Texto secundario, enlaces, etiquetas de estado: 4,7:1 o más en todos los casos (AA exige 4,5:1).
- Test de Rust `both_themes_keep_every_pair_of_colours_legible`: lee los tokens de `page.html` y falla si algún par
  baja de su mínimo en cualquiera de los dos temas, o si «seguir al sistema» y «elegir oscuro» dejaran de ser los
  mismos colores.

### Lo que no se pudo arreglar

En **protanopía**, el verde de Papel y el rojo de Papel casi no se distinguen (ΔE 3; en Tinta, 31). Se separaron por
luminosidad todo lo que cabe sin dejar de ser verde y rojo. Por eso ningún estado depende solo del color: lo atrasado
lleva «!» y el texto «vencida», lo confirmado lleva «✓» y el texto «Aprobada», y las notas del repaso llevan su número.

## El mismo sistema en la intro

La intro (`jobbot-showcase`) usa estos tokens: su noche es Tinta, la mañana de la última pantalla es Papel, el dorado
es «te toca» y el verde es «confirmado». El reloj de 24 horas del tablero es el mismo objeto de la intro, y los gestos
de movimiento también (el sol que abre una vista, el viaje entre pestañas, la barra que se llena al aprobar).
