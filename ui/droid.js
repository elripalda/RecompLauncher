// Android never shows the desktop view: Big Picture mode or the phone layout covers it. The page is
// marked before anything is drawn (this runs from <head>), so the desktop view doesn't show for a
// moment at start-up either (phone.css hides it).
if (/\bAndroid\b/.test(navigator.userAgent)) document.documentElement.classList.add('droid');
