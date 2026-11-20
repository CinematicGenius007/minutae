declare module "*.module.css" {
  const classes: Readonly<Record<string, string>>;
  export default classes;
}
declare module "*.css";
declare module "*?url" {
  const url: string;
  export default url;
}
