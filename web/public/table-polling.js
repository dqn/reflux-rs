(function() {
  var container = document.getElementById('table-data');
  if (!container) return;

  var username = container.dataset.username;
  var lampStyles = container.dataset.lampStyles;
  if (!username || !lampStyles) return;

  var LAMP_STYLES = JSON.parse(lampStyles);
  var lastPoll = new Date().toISOString();

  function updateCell(key, lamp) {
    var cells = document.querySelectorAll('.lamp-cell[data-key="' + key + '"]');
    cells.forEach(function(cell) {
      var style = LAMP_STYLES[lamp] || LAMP_STYLES["NO PLAY"];
      cell.dataset.lamp = lamp;
      cell.style.color = style.color;
      if (style.background.indexOf("linear-gradient") === 0) {
        cell.style.backgroundImage = style.background;
        cell.style.backgroundColor = "";
      } else {
        cell.style.backgroundColor = style.background;
        cell.style.backgroundImage = "";
      }
      cell.style.border = style.border || "";
    });
  }

  function poll() {
    fetch("/api/lamps/updated-since?since=" + encodeURIComponent(lastPoll) + "&user=" + encodeURIComponent(username))
      .then(function(res) { return res.json(); })
      .then(function(data) {
        if (data.lamps && data.lamps.length > 0) {
          data.lamps.forEach(function(l) {
            updateCell(l.songId + ":" + l.difficulty, l.lamp);
          });
          lastPoll = new Date().toISOString();
        }
      })
      .catch(function(err) { console.error('Polling error:', err); });
  }

  setInterval(poll, 5000);
})();
