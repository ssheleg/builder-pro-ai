import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { OrchdDownBanner } from "../OrchdDownBanner";
import { ServersTab } from "./ServersTab";
import { ToolsBrowser } from "./ToolsBrowser";
import { ConnectorsTab } from "./ConnectorsTab";
import { theme } from "../../theme";

type TabKey = "servers" | "tools" | "connectors" | "log" | "artifacts" | "skills";

const TABS: { key: TabKey; label: string }[] = [
  { key: "servers", label: "Серверы" },
  { key: "tools", label: "Инструменты" },
  { key: "connectors", label: "Коннекторы" },
  { key: "log", label: "Журнал" },
  { key: "artifacts", label: "Артефакты" },
  { key: "skills", label: "Навыки" },
];

/** Tabs not yet built (S-EXT §8: Журнал/Артефакты/Навыки land in later tasks — T17/T18 per the
 * brief; «Коннекторы» shipped in T13b). Rendering a stub rather than omitting the tab keeps the
 * full planned surface visible/navigable now, honestly labelled as not-yet-built. */
const STUB_TABS = new Set<TabKey>(["log", "artifacts", "skills"]);

const panelStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  color: theme.colors.text,
};

const headerStyle: CSSProperties = {
  padding: "10px 16px",
  borderBottom: `1px solid ${theme.colors.border}`,
};

const tabBarStyle: CSSProperties = {
  display: "flex",
  gap: 4,
  padding: "6px 16px",
  borderBottom: `1px solid ${theme.colors.border}`,
};

const contentStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: 16,
};

function ComingSoonStub(props: { label: string }): JSX.Element {
  return (
    <div data-testid="ext-tab-stub" style={{ color: theme.colors.textDim, fontSize: 13 }}>
      «{props.label}» — скоро
    </div>
  );
}

/**
 * «Расширения» top-level view (S-EXT §8, T8): the MCP servers/tools/connectors/skills management
 * panel, mirrors `ProjectPanel`'s tab pattern (`TABS: {key,label}[]`, ONE tab mounted at a time,
 * `activeTab` local state) + its honest-degradation discipline (the shared `<OrchdDownBanner/>`
 * renders above the tab bar whenever `orchdDown`, matching `ProjectPanel`'s placement exactly).
 *
 * «Серверы» (`ServersTab`), «Инструменты» (`ToolsBrowser`, T8) and «Коннекторы» (`ConnectorsTab`,
 * T13b) are built — the remaining three tabs render a `ComingSoonStub` (see `STUB_TABS`) rather
 * than being omitted, so the full planned surface is visible/navigable now.
 *
 * On mount, eagerly `refreshMcpServers()` — mirrors `ProjectPanel`'s own mount-fetch effect
 * (spec §10 "honest state, always": the server list must not silently stay `[]` just because
 * nothing else happened to refresh it yet).
 */
export function ExtPanel(): JSX.Element {
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshMcpServers = useAppStore((s) => s.refreshMcpServers);

  const [activeTab, setActiveTab] = useState<TabKey>("servers");

  useEffect(() => {
    void refreshMcpServers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div data-testid="ext-panel" style={panelStyle}>
      <div style={headerStyle}>
        <div style={{ fontSize: 16, fontWeight: 700 }}>Расширения</div>
      </div>

      {orchdDown && <OrchdDownBanner />}

      <div role="tablist" style={tabBarStyle}>
        {TABS.map((t) => (
          <button
            key={t.key}
            type="button"
            role="tab"
            aria-selected={activeTab === t.key}
            data-testid={`ext-tab-${t.key}`}
            onClick={() => setActiveTab(t.key)}
            style={{
              padding: "6px 10px",
              fontSize: 13,
              border: "none",
              borderBottom: activeTab === t.key ? `2px solid ${theme.colors.accent}` : "2px solid transparent",
              background: "transparent",
              color: activeTab === t.key ? theme.colors.text : theme.colors.textDim,
              cursor: "pointer",
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div style={contentStyle}>
        {activeTab === "servers" && <ServersTab />}
        {activeTab === "tools" && <ToolsBrowser />}
        {activeTab === "connectors" && <ConnectorsTab />}
        {STUB_TABS.has(activeTab) && (
          <ComingSoonStub label={TABS.find((t) => t.key === activeTab)!.label} />
        )}
      </div>
    </div>
  );
}
