/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        carambola: {
          yellow: "#FBBF24",
          lime: "#84CC16",
          cyan: "#06B6D4",
          dark: "#0F172A",
        },
      },
    },
  },
  plugins: [],
}

