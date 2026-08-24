export function projectNameError(value: string): string | null {
  const name = value.trim();
  if (!name) return "请输入项目名称。";
  if (name === "." || name === ".." || /[\\/\0]/u.test(name))
    return "项目名称不能包含路径分隔符。";
  return null;
}
