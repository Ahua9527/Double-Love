// CSS模块类型声明
declare module '*.css' {
  const classes: { readonly [key: string]: string };
  export default classes;
}
