import { useState, useEffect, useCallback } from "react";
import KinodeClientApi from "@kinode/client-api";
import "./App.css";
import { GetRandom } from "./types/Rng";
import { useWindowWidth } from '@react-hook/window-size'
import useRandomStore from "./store/rng";

const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

const PROXY_TARGET = `${(import.meta.env.VITE_NODE_URL || "http://localhost:8080")}${BASE_URL}`;

// This env also has BASE_URL which should match the process + package name
const WEBSOCKET_URL = import.meta.env.DEV
  ? `${PROXY_TARGET.replace('http', 'ws')}`
  : undefined;

function App() {
  const { randoms, newRandom, theme, changeTheme, set } = useRandomStore();

  const [target, setTarget] = useState("");
  const [context, setContext] = useState("");
  const [range, setRange] = useState({min: 0, max: 1});
  const [nodeConnected, setNodeConnected] = useState(true);
  const [api, setApi] = useState<KinodeClientApi | undefined>();

  const width = useWindowWidth();

  useEffect(() => {
    fetch(`${BASE_URL}/randoms`)
      .then((response) => response.json())
      .then((data) => {
        set({ randoms: data.reverse() || [] });
      })
      .catch((error) => console.error(error));

    console.log('WEBSOCKET URL', WEBSOCKET_URL)
    if (window.our?.node && window.our?.process) {
      const api = new KinodeClientApi({
        uri: WEBSOCKET_URL,
        nodeId: window.our.node,
        processId: window.our.process,
        onOpen: (_event, _api) => {
          console.log("Connected to Kinode");
          // api.send({ data: "Hello World" });
        },
        onMessage: (json, _api) => {
          console.log('WEBSOCKET MESSAGE', json)
          try {
            const data = JSON.parse(json);
            console.log("WebSocket received message", data);
            const messageType = data.kind;
            console.log(messageType)
            if (!messageType) return;

            if (messageType === "NewRandom") {
              newRandom(data.data);
            }
          } catch (error) {
            console.error("Error parsing WebSocket message", error);
          }
        },
      });
      
      setApi(api);
    } else {
      setNodeConnected(false);
    }
  }, []);

  const getRandom = useCallback(
    async (event) => {
      event.preventDefault();
      if (!api || !target || !range) return;
      const data = {
           target,
           range,
           context,
        } as GetRandom;

      try {
        const result = await fetch(`${BASE_URL}/randoms`, {
          method: "POST",
          body: JSON.stringify(data),
        });

        if (!result.ok) throw new Error("HTTP request failed");
        setTarget('')
        setContext('')
      } catch (error) {
        console.error(error);
      }
    },
    [api, target, context, range, randoms, set]
  )
    

  return (
    <main data-theme={theme}>
      <div style={{ position: "absolute", top: 12, left: 12}}> 
        <strong>kinode_rng::{window.our?.node}</strong>
      </div>
      {!nodeConnected && (
        <div className="node-not-connected">
          <h2 style={{ color: "red" }}>Node not connected</h2>
          <h4>
            You need to start a node at {PROXY_TARGET} before you can use this UI
            in development.
          </h4>
        </div>
      )}
      <h1><strong style={{fontSize: '64px'}}>#</strong></h1>
      <div>
        <div
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: 'center',
                padding: '1em',
                justifyContent: "space-between"
              }}
            >
          <form
            onSubmit={getRandom}
            style={{ display: "flex", flexDirection: "column" }}
          >
            <label
              style={{ fontWeight: 600, alignSelf: "flex-start" }}
              htmlFor="target"
            >
              Target Node
            </label>
            <input
              style={{
                padding: "0.25em 0.5em",
                fontSize: "1em",
                marginBottom: "1em",
              
              }}
              type="text"
              id="target"
              value={target}
              onChange={(event) => setTarget(event.target.value)}
            />
            {/* <label
              style={{ fontWeight: 600, alignSelf: "center" }}
              htmlFor="range"
            >
              Range
            </label> */}
            <div style={{display: 'flex', marginTop: "1em", justifyContent: 'center'}}>
             <input
              style={{
                padding: "0.25em 0.5em",
                fontSize: "1em",
                width: '60px',
                marginBottom: "1em",
              }}
              type="number"
              min="0"
              id="min"
              value={range.min}
              onChange={(event) => setRange({max: range.max, min: event.target.valueAsNumber})}
            />..
             <input
              style={{
                padding: "0.25em 0.5em",
                fontSize: "1em",
                width: '60px',
                marginBottom: "1em",
              }}
              type="number"
              id="max"
              min={range.min+1}
              value={range.max}
              onChange={(event) => setRange({min: range.min, max: event.target.valueAsNumber})}
            />
            </div>
            <label
              style={{ fontWeight: 600, alignSelf: "flex-start" }}
              htmlFor="context"
            >
              Context
            </label>
            <input
              style={{
                padding: "0.25em 0.5em",
                fontSize: "1em",
                marginBottom: "1em",
              }}
              type="text"
              id="context"
              value={context}
              onChange={(event) => setContext(event.target.value)}
            />
            <button 
             style={{ 
              padding: "0.25em 0.25em",
              fontSize: "1em",
              width: width > 480 ?'450px' : '350px',
              alignSelf: 'center'
            }}type="submit"> Send</button>
          </form>
        </div>
        { randoms.length > 0 && <div className='results'>
          <table style={{minWidth: width > 480 ? '450px' : '350px'}}>
            <thead>
              <tr>
                <th colSpan={6} /> 
              </tr>
              </thead>
            <tbody>
              <tr>
                <th>source</th>       
                <th>range</th>
                <th>value</th>
                <th>target</th>
                <th>context</th>         
                {width > 450 && <th>time</th>}
              </tr>
              {randoms?.map((r,i) => {
                var when = new Date(r.timestamp);
                return (
                  <tr>
                    <td>
                       {r.msg_source}
                    </td>
                    <td>
                      {`[${r.range[0]}..${r.range[1]}]`}
                    </td>
                    <td>
                      {r.value}
                    </td>
                    <td>
                       {r.rng_source}
                    </td>
                    <td style={{minWidth: '8vw', maxWidth:  '42vw'}}>
                        {r.context || '#'}
                    </td> 
                    {width > 450 &&
                    <td>
                        {`${when.toLocaleTimeString("en-US")}`}   <br></br>
                        {`${when.toLocaleDateString("en-US")}`}  
                    </td>}
                  </tr>
                )}
              )}
            </tbody>
          </table>
        </div>
        }
      </div>
      <div className='bottom'>
        <div
        className='themeButton'
        onClick={() => changeTheme()} />
      </div>
    </main>
  );
}

export default App;
