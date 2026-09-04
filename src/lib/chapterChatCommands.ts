/** Workspace 细纲行 → 全局 Chat 的固定指令（须命中 global_chat 意图关键词）。 */

export function chatCmdGenerateOutline(chapterN: number): string {
  return `生成本章细纲（第${chapterN}章）。调用 generate_detailed_outline（node_id 可省略，用当前选中章）。材料由工具组装，不要 get_tree。细纲完成后用一句话结束。`;
}

export function chatCmdGenerateChapter(chapterN: number): string {
  return `生成第${chapterN}章正文。读一次 get_chapter_write_context，细纲空则 generate_detailed_outline，再 set_chapter_content 一次后立刻用一句话结束。禁止 get_chapter_content。禁止反复 get/set 同一章。`;
}

export function chatCmdRefineChapter(chapterN: number): string {
  return `精修第${chapterN}章正文。先 get_chapter_write_context，再 get_chapter_content 读旧稿后改，set_chapter_content 一次后立刻用一句话结束。必须在现有稿上改，禁止另起炉灶。`;
}
