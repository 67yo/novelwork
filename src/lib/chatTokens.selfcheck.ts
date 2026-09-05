/** ponytail: chat token K/M format. Run: npx tsx src/lib/chatTokens.selfcheck.ts */
import { formatTokenAmount, readMessageTokens } from "./chatTokens";

if (formatTokenAmount(0) !== "0") throw new Error("0");
if (formatTokenAmount(999) !== "999") throw new Error("999");
if (formatTokenAmount(1000) !== "1K") throw new Error("1K");
if (formatTokenAmount(1530) !== "1.53K") throw new Error("1.53K");
if (formatTokenAmount(12_000) !== "12K") throw new Error("12K");
if (formatTokenAmount(12_500) !== "12.5K") throw new Error("12.5K");
if (formatTokenAmount(1_000_000) !== "1M") throw new Error("1M");
if (formatTokenAmount(1_250_000) !== "1.25M") throw new Error("1.25M");
if (formatTokenAmount(-3) !== "0") throw new Error("neg");
const fromSnake = readMessageTokens({ prompt_tokens: 1200, completion_tokens: 80 });
if (fromSnake.prompt !== 1200 || fromSnake.completion !== 80) throw new Error("snake");
const fromCamel = readMessageTokens({ promptTokens: 50, completionTokens: 9 });
if (fromCamel.prompt !== 50 || fromCamel.completion !== 9) throw new Error("camel");
if (readMessageTokens({}).prompt !== 0) throw new Error("empty");
console.log("chatTokens.selfcheck ok");
