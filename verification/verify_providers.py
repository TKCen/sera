import asyncio
from playwright.async_api import async_playwright, expect

async def verify_providers():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True)
        # Mock auth token by setting it in sessionStorage
        context = await browser.new_context(viewport={'width': 1280, 'height': 800})
        page = await context.new_page()

        try:
            # Go to the login page first or inject session
            await page.goto("http://localhost:5173/providers")

            # Since ProtectedRoute might redirect to /login, let's wait a bit and check URL
            await asyncio.sleep(2)
            print(f"Current URL: {page.url}")

            if "/login" in page.url:
                print("Redirected to login. Attempting to bypass auth...")
                # Try to inject a fake session token
                await page.evaluate("window.sessionStorage.setItem('sera_session', 'fake-token')")
                await page.evaluate("window.sessionStorage.setItem('sera_user', JSON.stringify({id: '1', email: 'test@example.com', name: 'Test User'}))")
                await page.goto("http://localhost:5173/providers")
                await asyncio.sleep(2)
                print(f"URL after bypass: {page.url}")

            # Take a screenshot of the main page
            await page.screenshot(path="verification/providers_page.png")
            print("Main page screenshot saved.")

            # Check for heading
            # await expect(page.locator("h1")).to_have_text("Providers", timeout=10000)

            # Try to open the activation dialog if there are available templates
            templates = page.locator("button.sera-card-static")
            if await templates.count() > 0:
                print(f"Found {await templates.count()} templates. Clicking first...")
                await templates.first.click()
                # Wait for dialog
                await asyncio.sleep(1)
                await page.screenshot(path="verification/activation_dialog.png")
                print("Activation dialog screenshot saved.")
            else:
                print("No templates found in this environment.")

        except Exception as e:
            print(f"Error during verification: {e}")
            # Take a screenshot anyway if possible
            await page.screenshot(path="verification/error.png")
        finally:
            await browser.close()

if __name__ == "__main__":
    asyncio.run(verify_providers())
