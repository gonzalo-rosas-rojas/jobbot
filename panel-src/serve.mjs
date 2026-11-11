// Kept so the v2 tests that spawn `node serve.mjs <port>` keep working until T22; the server lives in tools/serve.mjs.
import { main } from "./tools/serve.mjs";
await main(process.argv.slice(2));
