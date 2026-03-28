from playwright.sync_api import sync_playwright

def verify_frontend():
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        # Mock auth token
        context = browser.new_context()
        page = context.new_page()

        page.route("**/api/auth/session", lambda route: route.fulfill(status=200, json={"user": {"id": "1", "roles": ["admin"]}}))
        page.route("**/api/agents", lambda route: route.fulfill(status=200, json=[{"id": "agent-1", "name": "test-agent", "status": "running"}]))
        page.route("**/api/sessions", lambda route: route.fulfill(status=200, json=[]))

        # We first need to set sessionStorage which must be done on the same origin.
        page.goto("http://localhost:5173/")
        page.evaluate("sessionStorage.setItem('sera_access_token', 'mock_token');")

        page.goto("http://localhost:5173/chat")

        # Give React some time to render
        page.wait_for_timeout(2000)

        page.screenshot(path="chat_page.png", full_page=True)
        print("Screenshot taken: chat_page.png")

if __name__ == "__main__":
    verify_frontend()
