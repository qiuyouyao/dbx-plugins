import JsonFormatter from "./JsonFormatter.svelte";
import PasswordGenerator from "./PasswordGenerator.svelte";

export const tools = [
  { id: "json", labelKey: "toolJson", component: JsonFormatter },
  { id: "password", labelKey: "toolPassword", component: PasswordGenerator },
];
