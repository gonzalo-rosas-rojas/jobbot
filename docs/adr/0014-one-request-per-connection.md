# 0014. One request per connection in the panel

Status: accepted

## Context

Now and then the panel's page stayed half loaded: one request (a font, a script) got no answer
for minutes while the server answered every new connection in milliseconds.

The cause is in the HTTP library, tiny_http 0.12. It hands each connection to a pool of threads.
When the pool believes a thread is idle it queues the connection and wakes one thread, but its
count of idle threads still includes a thread that was woken a moment ago and has not taken its
connection yet. When more connections arrive together than threads are really idle (a page load
opens six or more), one of them stays in the queue with no thread to read it, until some other
connection ends. Connections were kept alive, so that took as long as the browser kept them.

Measured with a burst of connections that each ask as a browser does: bursts of two to four never
lose one, a burst of five or more does, and the one left out is answered the moment another
connection hangs up. `every_connection_of_a_burst_is_answered` in
`crates/jobbot-panel/src/server.rs` fails on its first round without this decision.

The same reading of the library showed a second way to hold the panel. tiny_http reads what is
left of a request's body off the socket when the request ends, with no timeout. A request that
was refused on its head and neither sent its body nor hung up kept the panel's one serving thread
waiting for it.

## Decision

- Every answer to a request whose body is settled (none declared, or read whole) says
  `Connection: close`. That is every request the page makes. The browser hangs up after each
  answer, so a thread comes free within milliseconds and nothing is left waiting in the queue.
- tiny_http drops a `Connection` header that the application adds, so the panel writes these
  responses itself (`on_the_wire`): status line, `Date`, the reply's headers, `Content-Length`.
  A header holding a control character is left out and logged. They carry no `Server` header;
  tiny_http added one naming itself.
- A request that still owes body when its answer is ready (refused on its head, too big, turned
  away) is answered through tiny_http's own `respond`, on a thread of its own. That is the one
  call that writes the answer before waiting for the owed bytes, and the wait is not the serving
  thread's. These answers do not close the connection; the page never makes such a request.

## Consequences

- No keep-alive: each request of the page opens a connection to `127.0.0.1`. On the loopback
  that costs well under a millisecond, and the page makes a few dozen requests.
- The pool's miscount is still there. What it can cost now is the few milliseconds until some
  connection of the burst closes, not minutes.
- A panic while a request is handled is still answered by tiny_http (a `500` that keeps the
  connection open).
- The threads that wait for owed bytes are not capped: each lasts as long as its connection, for
  which tiny_http already keeps a thread, and a page in the browser gets only a handful of
  connections to one host. A cap would need something to do past it, and every way to end such a
  request waits for its client: with a cap, a few lingering connections would hold the serving
  thread again. Only when the machine has no thread left to give does the serving thread wait.

## Alternatives

- A patched copy of tiny_http: a fork to keep in step with its upstream, for a server of one user.
- Another HTTP library (hyper): the right move if the panel ever needs more of HTTP than this.
  It replaces the whole socket side and its tests, which this bug alone does not justify.
- Answering in HTTP/1.0 so the library closes the connection itself: it works by a side effect
  of how the library prints a response, which a reader of the code could not be expected to know.
