import { useEffect, useState } from "react";
import { api, onRunDone, onRunError, onRunProgress } from "./api";
import { DeckFile, DeckInfo, PoolInfo, ProgressPayload, RunMeta, RunResult, UpdateStatus } from "./types";
import { DecksView } from "./views/Decks";
import { RunView } from "./views/Run";
import { ResultsView } from "./views/Results";
import { MetaView } from "./views/Meta";
import { HistoryView } from "./views/History";
import { GlossaryView } from "./views/Glossary";

type View = "decks" | "run" | "results" | "meta" | "history" | "glossary";

export default function App() {
  const [view, setView] = useState<View>("decks");
  const [pool, setPool] = useState<PoolInfo | null>(null);
  const [poolErr, setPoolErr] = useState("");
  const [decks, setDecks] = useState<DeckFile[]>([]);
  const [currentDeck, setCurrentDeck] = useState<DeckInfo | null>(null);
  const [currentText, setCurrentText] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ProgressPayload | null>(null);
  const [runError, setRunError] = useState("");
  const [result, setResult] = useState<RunResult | null>(null);
  const [runs, setRuns] = useState<RunMeta[]>([]);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);

  const refreshDecks = () => void api.listDecks().then(setDecks).catch(() => {});
  const refreshRuns = () => void api.listRuns().then(setRuns).catch(() => {});

  useEffect(() => {
    api.poolStatus().then(setPool).catch((e) => setPoolErr(String(e)));
    refreshDecks();
    refreshRuns();
    // Non-blocking update poll; a dismissed version is remembered so the
    // banner does not reappear until a newer one ships.
    void api
      .checkUpdate()
      .then((u) => {
        if (u.update_available && localStorage.getItem("dismissedUpdate") !== u.latest) {
          setUpdate(u);
        }
      })
      .catch(() => {});
    const subs = [
      onRunProgress((p) => {
        setRunning(true);
        setProgress(p);
      }),
      onRunDone((r) => {
        setRunning(false);
        setResult(r);
        setRunError("");
        refreshRuns();
        setView("results");
      }),
      onRunError((e) => {
        setRunning(false);
        setRunError(e);
      }),
    ];
    return () => {
      subs.forEach((s) => void s.then((un) => un()));
    };
  }, []);

  const startRunView = () => setView("run");

  const openRun = (file: string) => {
    void api.loadRun(file).then((r) => {
      setResult(r);
      setView("results");
    });
  };

  const nav: { id: View; label: string }[] = [
    { id: "decks", label: "Decks" },
    { id: "run", label: "Run" },
    { id: "results", label: "Results" },
    { id: "meta", label: "Meta" },
    { id: "history", label: "History" },
    { id: "glossary", label: "Glossary" },
  ];

  return (
    <>
      <div className="sidebar">
        <div className="brand">
          TOLARIA<span>.</span>
        </div>
        <div className="tagline">a time bubble for your decklist</div>
        {nav.map((n) => (
          <button
            key={n.id}
            className={`nav-item${view === n.id ? " active" : ""}`}
            onClick={() => setView(n.id)}
          >
            <span>{n.label}</span>
            {n.id === "run" && running ? <span className="dot" /> : null}
          </button>
        ))}
        <div className="foot">
          {pool
            ? `${pool.cards.toLocaleString()} cards (${pool.source})`
            : poolErr
              ? `card pool error: ${poolErr}`
              : "loading card pool..."}
          {currentDeck ? (
            <div style={{ marginTop: 4 }}>deck: {currentDeck.name}</div>
          ) : null}
        </div>
      </div>
      <div className="main">
        {update ? (
          <div className="update-banner">
            <span>
              Tolaria {update.latest} is available (you have {update.current}).
            </span>
            <span className="update-actions">
              <button className="update-get" onClick={() => void api.openReleasesPage()}>
                get it
              </button>
              <button
                className="update-dismiss"
                onClick={() => {
                  if (update.latest) localStorage.setItem("dismissedUpdate", update.latest);
                  setUpdate(null);
                }}
              >
                later
              </button>
            </span>
          </div>
        ) : null}
        {view === "decks" ? (
          <DecksView
            decks={decks}
            refreshDecks={refreshDecks}
            current={currentDeck}
            setCurrent={setCurrentDeck}
            currentText={currentText}
            setCurrentText={setCurrentText}
            goRun={startRunView}
          />
        ) : null}
        {view === "run" ? (
          <RunView
            currentDeck={currentDeck}
            currentText={currentText}
            decks={decks}
            running={running}
            progress={progress}
            error={runError}
          />
        ) : null}
        {view === "results" ? <ResultsView result={result} /> : null}
        {view === "meta" ? <MetaView /> : null}
        {view === "history" ? <HistoryView runs={runs} openRun={openRun} /> : null}
        {view === "glossary" ? <GlossaryView /> : null}
      </div>
    </>
  );
}
