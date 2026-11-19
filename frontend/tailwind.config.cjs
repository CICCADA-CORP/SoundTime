const tokens = [
  "background", "foreground", "card", "card-foreground", "popover", "popover-foreground",
  "primary", "primary-foreground", "secondary", "secondary-foreground", "accent",
  "accent-foreground", "muted", "muted-foreground", "destructive", "destructive-foreground",
  "border", "input", "ring", "sidebar",
];

/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/**/*.{html,js,svelte,ts}"],
  theme: {
    extend: {
      colors: Object.fromEntries(tokens.map((name) => [name, `hsl(var(--${name}) / <alpha-value>)`])),
      borderRadius: { lg: "var(--radius)", md: "calc(var(--radius) - 2px)", sm: "calc(var(--radius) - 4px)" },
    },
  },
};
