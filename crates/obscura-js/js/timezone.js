// Context-scoped timezone emulation. Runs once per V8 realm, never mutates TZ or
// V8's process-wide date cache. The original builtins remain usable for UTC.
(() => {
  if (typeof globalThis.__obscura_setTimezoneOverride === 'function') return;
  const NativeDate = globalThis.Date;
  const NativeFormatter = Intl.DateTimeFormat;
  const original = {};
  for (const name of [
    'getTimezoneOffset','getFullYear','getMonth','getDate','getDay',
    'getHours','getMinutes','getSeconds','toString','toDateString','toTimeString',
    'toLocaleString','toLocaleDateString','toLocaleTimeString'
  ]) original[name] = NativeDate.prototype[name];

  let zone = null, formatter = null;
  const getParts = date => {
    const parts = Object.create(null);
    for (const part of formatter.formatToParts(date)) {
      if (part.type !== 'literal') parts[part.type] = Number(part.value);
    }
    return parts;
  };
  const localParts = date => zone && Number.isFinite(date.getTime()) ? getParts(date) : null;
  const offset = date => {
    const parts = localParts(date);
    if (!parts) return original.getTimezoneOffset.call(date);
    const local = NativeDate.UTC(parts.year, parts.month - 1, parts.day,
      parts.hour, parts.minute, parts.second, date.getUTCMilliseconds());
    return Math.round((date.getTime() - local) / 60000);
  };
  const localConstructor = args => {
    const padded = args.slice();
    while (padded.length < 7) padded.push(padded.length === 2 ? 1 : 0);
    const utc = NativeDate.UTC(...padded.slice(0, 7));
    if (!Number.isFinite(utc)) return new NativeDate(NaN);
    let guess = new NativeDate(utc);
    let candidate = utc + offset(guess) * 60000;
    guess = new NativeDate(candidate);
    candidate = utc + offset(guess) * 60000;
    return new NativeDate(candidate);
  };
  const getters = {
    getFullYear: p => p.year, getMonth: p => p.month - 1,
    getDate: p => p.day, getHours: p => p.hour,
    getMinutes: p => p.minute, getSeconds: p => p.second,
    getDay: p => new NativeDate(NativeDate.UTC(p.year, p.month - 1, p.day)).getUTCDay(),
  };
  for (const [name, select] of Object.entries(getters)) {
    NativeDate.prototype[name] = function() {
      const parts = localParts(this);
      return parts ? select(parts) : original[name].call(this);
    };
  }
  NativeDate.prototype.getTimezoneOffset = function() { return offset(this); };
  const months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
  const weekdays = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  const pad = number => String(number).padStart(2, '0');
  const describe = date => {
    const p = localParts(date);
    if (!p) return null;
    const minutes = -offset(date);
    const sign = minutes < 0 ? '-' : '+';
    const z = Math.abs(minutes);
    const long = new NativeFormatter('en-US', {
      timeZone: zone, timeZoneName: 'long',
    }).formatToParts(date).find(part => part.type === 'timeZoneName')?.value || zone;
    const day = weekdays[new NativeDate(NativeDate.UTC(p.year, p.month - 1, p.day)).getUTCDay()];
    return {
      date: day + ' ' + months[p.month - 1] + ' ' + pad(p.day) + ' ' + p.year,
      time: pad(p.hour) + ':' + pad(p.minute) + ':' + pad(p.second)
        + ' GMT' + sign + pad(Math.trunc(z / 60)) + pad(z % 60) + ' (' + long + ')',
    };
  };
  for (const name of ['toString', 'toDateString', 'toTimeString']) {
    NativeDate.prototype[name] = function() {
      const parts = describe(this);
      if (!parts) return original[name].call(this);
      if (name === 'toDateString') return parts.date;
      if (name === 'toTimeString') return parts.time;
      return parts.date + ' ' + parts.time;
    };
  }
  for (const name of ['toLocaleString','toLocaleDateString','toLocaleTimeString']) {
    NativeDate.prototype[name] = function(locales, options) {
      if (!zone || options?.timeZone) return original[name].call(this, locales, options);
      return original[name].call(this, locales, { ...options, timeZone: zone });
    };
  }
  function ContextDateTimeFormat(locales, options) {
    if (!zone || options?.timeZone) return new NativeFormatter(locales, options);
    return new NativeFormatter(locales, { ...options, timeZone: zone });
  }
  ContextDateTimeFormat.prototype = NativeFormatter.prototype;
  Object.setPrototypeOf(ContextDateTimeFormat, NativeFormatter);
  Intl.DateTimeFormat = ContextDateTimeFormat;
  globalThis.Date = new Proxy(NativeDate, {
    apply() { return (new NativeDate()).toString(); },
    construct(target, args, newTarget) {
      if (!zone || args.length < 2) return Reflect.construct(target, args, newTarget);
      return Reflect.construct(target, [localConstructor(args).getTime()], newTarget);
    },
  });
  Object.defineProperty(globalThis, '__obscura_setTimezoneOverride', {
    configurable: false,
    value(timeZone) {
      if (timeZone) {
        // Reject invalid IANA IDs before mutating any realm state.
        const next = new NativeFormatter('en-US', {
          timeZone, year: 'numeric', month: '2-digit', day: '2-digit',
          hour: '2-digit', minute: '2-digit', second: '2-digit',
          hourCycle: 'h23', calendar: 'gregory', numberingSystem: 'latn',
        });
        formatter = next;
        zone = next.resolvedOptions().timeZone;
      } else {
        zone = null;
        formatter = null;
      }
      globalThis.__obscura_tz = zone || '';
    },
  });
})();
