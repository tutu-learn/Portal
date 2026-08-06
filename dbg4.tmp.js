const { chromium } = require('playwright');

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.log('[pageerror]', String(e).slice(0, 300)));

  await page.goto('http://localhost:8000/login');
  await page.locator('#login_email').fill('Administrator');
  await page.locator('#login_password').fill('admin');
  await page.locator('.btn-login, button[type=submit]').first().click();
  await page.waitForTimeout(9000);

  // open dropdown, click "Desktop"
  await page.locator('.body-sidebar .sidebar-header, .body-sidebar > div').first().click();
  await page.waitForTimeout(1000);
  await page.locator('text=Desktop').first().click();
  await page.waitForTimeout(5000);
  console.log('url:', page.url());
  await page.screenshot({ path: '/tmp/desktop_page.png' });
  const txt = await page.evaluate(() => document.body.innerText);
  console.log('desktop has Sebrus:', /Sebrus/.test(txt));
  await browser.close();
})();
