import {
  bodySuggestContext,
  insertBodySuggestion,
  nonEmptyLineIndexes,
  replaceBodyLine,
  splitBodyLines,
} from "./chapterParagraphs";

const lines = splitBodyLines("甲\n\n乙丙\n丁");
if (lines.length !== 4 || lines[0] !== "甲" || lines[2] !== "乙丙") {
  throw new Error("splitBodyLines");
}

const replaced = replaceBodyLine("甲\n乙\n丙", 1, "乙改\n乙续");
if (replaced !== "甲\n乙改\n乙续\n丙") throw new Error(`replace multi: ${replaced}`);

const trimmed = replaceBodyLine("甲\n  \n丙", 1, "  ");
if (splitBodyLines(trimmed)[1] !== "") throw new Error("replace empty");

const idxs = nonEmptyLineIndexes("甲\n\n乙");
if (idxs.join(",") !== "0,2") throw new Error(`idxs ${idxs}`);

const ctxSrc = "上一段\n正在写的半句";
const ctx = bodySuggestContext(ctxSrc, ctxSrc.length);
if (ctx.prevParagraph !== "上一段" || ctx.current !== "正在写的半句") {
  throw new Error(`suggest ctx ${JSON.stringify(ctx)}`);
}
const ctxMid = bodySuggestContext("上一段\n当前段全文", 5);
if (ctxMid.current !== "当前段全文" || ctxMid.prevParagraph !== "上一段") {
  throw new Error(`suggest ctx mid ${JSON.stringify(ctxMid)}`);
}
const ctxEmpty = bodySuggestContext("上一段\n\n", 5);
if (ctxEmpty.current !== "" || ctxEmpty.prevParagraph !== "上一段") {
  throw new Error(`suggest ctx empty ${JSON.stringify(ctxEmpty)}`);
}

const inserted = insertBodySuggestion("甲\n乙", 3, "丙");
if (inserted.text !== "甲\n乙\n丙" || inserted.cursor !== 5) {
  throw new Error(`insert ${JSON.stringify(inserted)}`);
}

console.log("chapterParagraphs.selfcheck ok");
