import { nonEmptyLineIndexes, replaceBodyLine, splitBodyLines } from "./chapterParagraphs";

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

console.log("chapterParagraphs.selfcheck ok");
