const vertical = (top: string, bottom: string) => ({
  type: "linear" as const,
  x: 0,
  y: 0,
  x2: 0,
  y2: 1,
  colorStops: [{ offset: 0, color: top }, { offset: 1, color: bottom }],
});

export const chartTheme = {
  axis: "#9a8d7f",
  grid: "rgba(150, 118, 86, .14)",
  accent: "#f4573a",
  areaGradient: vertical("rgba(244, 87, 58, .28)", "rgba(244, 87, 58, .01)"),
  barGradient: vertical("#ff9a76", "#f1442a"),
  heatmap: ["rgba(255, 154, 118, .10)", "#ffd3b8", "#ff9a76", "#f1442a", "#9c2a1c"],
  palette: ["#f4573a", "#ffa27a", "#f2b45a", "#8e6c8a", "#7fa58a", "#d9826b", "#8a9bb0"],
  tooltip: {
    backgroundColor: "rgba(255, 252, 247, .97)",
    borderColor: "rgba(150, 118, 86, .16)",
    textStyle: { color: "#2b2622", fontSize: 12 },
    extraCssText: "border-radius:12px;box-shadow:0 12px 32px rgba(120,80,40,.14);",
  },
};
