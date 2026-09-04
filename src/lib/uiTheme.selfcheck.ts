/** ponytail: ui theme parse. Run: npx tsx src/lib/uiTheme.selfcheck.ts */
import { parseUiThemePref, resolveUiDark } from "./uiTheme";

if (parseUiThemePref(null) !== "system") throw new Error("null → system");
if (parseUiThemePref("nope") !== "system") throw new Error("bad → system");
if (parseUiThemePref("dark") !== "dark") throw new Error("dark");
if (resolveUiDark("light", true)) throw new Error("light wins");
if (!resolveUiDark("dark", false)) throw new Error("dark wins");
if (!resolveUiDark("system", true)) throw new Error("system follows");
if (resolveUiDark("system", false)) throw new Error("system light");
console.log("uiTheme.selfcheck ok");
