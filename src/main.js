const { invoke } = window.__TAURI__.core;

// --- State Machine ---
const STATES = {
  LOGIN: 'LOGIN',
  DASHBOARD: 'DASHBOARD'
};

let currentState = STATES.LOGIN;
let currentRegion = 'ap';

// Cache for map and agent names from valorant-api.com
const valoApiCache = {
  maps: {},
  agents: {}
};

// --- DOM Elements ---
const overlay = document.getElementById('loading-overlay');
const overlayText = document.getElementById('loading-text');
const errorToast = document.getElementById('error-toast');

const containers = {
  [STATES.LOGIN]: document.getElementById('login-container'),
  [STATES.DASHBOARD]: document.getElementById('dashboard-container')
};

const loginContainer = document.getElementById('login-container');
const webviewLoginBtn = document.getElementById('webview-login-btn');
const regionSelect = document.getElementById('region');
const dashboardContainer = document.getElementById('dashboard-container');

// --- Utilities ---
function switchState(newState) {
  Object.values(containers).forEach(c => c.classList.add('hidden'));
  containers[newState].classList.remove('hidden');
  currentState = newState;
}

function showLoading(text = 'Loading...') {
  overlayText.innerText = text;
  overlay.classList.remove('hidden');
}

function hideLoading() {
  overlay.classList.add('hidden');
}

function showError(msg) {
  errorToast.innerText = msg;
  errorToast.classList.remove('hidden');
  setTimeout(() => {
    errorToast.classList.add('hidden');
  }, 4000);
}

// --- Data Fetching ---
async function prefetchValorantApiData() {
  try {
    const [mapsRes, agentsRes] = await Promise.all([
      fetch('https://valorant-api.com/v1/maps?language=zh-TW'),
      fetch('https://valorant-api.com/v1/agents?language=zh-TW&isPlayableCharacter=true')
    ]);
    
    if (mapsRes.ok) {
      const json = await mapsRes.json();
      json.data.forEach(m => {
        valoApiCache.maps[m.mapUrl] = m.displayName;
      });
    }
    
    if (agentsRes.ok) {
      const json = await agentsRes.json();
      json.data.forEach(a => {
        valoApiCache.agents[a.uuid] = {
          name: a.displayName,
          icon: a.displayIconSmall
        };
      });
    }
  } catch (e) {
    console.error("Failed to fetch valorant-api resources", e);
  }
}

// --- Dashboard Logic ---
async function loadDashboard() {
  switchState(STATES.DASHBOARD);
  showLoading('Loading Storefront...');
  
  // Load Store
  try {
    const uuids = await invoke('get_storefront', { region: currentRegion });
    const skinDetails = await invoke('get_skin_details', { 
      uuids: uuids, 
      language: "zh-TW" 
    });
    
    const storeGrid = document.getElementById('store-grid');
    storeGrid.innerHTML = '';
    
    skinDetails.forEach(skin => {
      const div = document.createElement('div');
      div.className = 'store-item';
      div.innerHTML = `
        <img src="${skin.display_icon || 'placeholder.png'}" alt="${skin.display_name}">
        <div class="item-name">${skin.display_name || 'Unknown Skin'}</div>
      `;
      storeGrid.appendChild(div);
    });
  } catch (e) {
    showError(e);
  }
  
  hideLoading();
  
  // Prefetch Maps and Agents for History
  await prefetchValorantApiData();
}

async function loadMatchHistory() {
  const historyList = document.getElementById('history-list');
  historyList.innerHTML = '<div style="text-align:center; padding: 20px;">Loading Match History...</div>';
  
  try {
    const historyRes = await invoke('get_match_history', { region: currentRegion });
    const subject = historyRes.Subject;
    const matches = historyRes.History || [];
    
    if (matches.length === 0) {
      historyList.innerHTML = '<div style="text-align:center;">No recent matches found.</div>';
      return;
    }

    // Promise.all for concurrent fetch of match details (M6 requirement)
    const matchPromises = matches.map(m => 
      invoke('get_match_details', { region: currentRegion, matchId: m.MatchID }).catch(e => null)
    );
    
    const matchDetails = await Promise.all(matchPromises);
    
    historyList.innerHTML = '';
    
    matchDetails.forEach(details => {
      if (!details || !details.matchInfo) return;
      
      const players = details.players || [];
      const myPlayer = players.find(p => p.subject === subject);
      if (!myPlayer) return;
      
      const mapName = valoApiCache.maps[details.matchInfo.mapId] || 'Unknown Map';
      const mode = details.matchInfo.queueID || 'Custom';
      
      const stats = myPlayer.stats || { kills: 0, deaths: 0, assists: 0 };
      const kda = `${stats.kills} / ${stats.deaths} / ${stats.assists}`;
      
      // Determine Win/Loss
      let result = 'draw';
      let resultText = 'Draw';
      const myTeam = details.teams?.find(t => t.teamId === myPlayer.teamId);
      if (myTeam) {
        if (myTeam.won) {
          result = 'win';
          resultText = 'Victory';
        } else {
          result = 'loss';
          resultText = 'Defeat';
        }
      }
      
      const agent = valoApiCache.agents[myPlayer.characterId?.toLowerCase()] || { name: 'Unknown', icon: '' };
      
      const div = document.createElement('div');
      div.className = `history-item ${result}`;
      div.innerHTML = `
        <img src="${agent.icon}" alt="${agent.name}" title="${agent.name}">
        <div>
          <div style="font-weight:bold;">${mapName}</div>
          <div style="font-size:0.8rem; color:var(--text-muted);">${mode.toUpperCase()}</div>
        </div>
        <div>${kda}</div>
        <div style="font-weight:bold;">${resultText}</div>
      `;
      
      historyList.appendChild(div);
    });
    
  } catch (e) {
    showError(e);
    historyList.innerHTML = '';
  }
}

// --- Event Listeners ---
document.getElementById('webview-login-btn').addEventListener('click', async () => {
  const region = regionSelect.value;
  
  try {
    webviewLoginBtn.disabled = true;
    webviewLoginBtn.textContent = 'Opening Riot Login...';
    
    // In Tauri v2, if a command returns an Enum, it serializes to a JS object
    // e.g. "Success" or { Error: "..." }
    const result = await invoke('start_webview_login');
    
    if (result === 'Success') {
      currentRegion = region;
      loadDashboard();
    } else if (result && result.Error) {
      alert(`Login failed: ${result.Error}`);
      webviewLoginBtn.disabled = false;
      webviewLoginBtn.textContent = 'Login with Riot';
    } else {
      alert(`Unexpected response: ${JSON.stringify(result)}`);
      webviewLoginBtn.disabled = false;
      webviewLoginBtn.textContent = 'Login with Riot';
    }
  } catch (error) {
    alert(`Login error: ${error}`);
    webviewLoginBtn.disabled = false;
    webviewLoginBtn.textContent = 'Login with Riot';
  }
});

document.getElementById('logout-btn').addEventListener('click', async () => {
  showLoading('Logging out...');
  try {
    await invoke('logout');
    switchState(STATES.LOGIN);
  } catch (e) {
    showError(e);
  }
  hideLoading();
});

// Tab Switching
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', (e) => {
    // Update active class on buttons
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    e.target.classList.add('active');
    
    // Switch panes
    document.querySelectorAll('.tab-pane').forEach(p => p.classList.add('hidden'));
    const targetId = e.target.getAttribute('data-target');
    document.getElementById(targetId).classList.remove('hidden');
    
    if (targetId === 'history-tab') {
      const historyList = document.getElementById('history-list');
      if (historyList.children.length === 0) {
        loadMatchHistory();
      }
    }
  });
});
