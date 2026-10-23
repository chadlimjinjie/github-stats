import { UserManager } from 'oidc-client-ts'

/** Origin of the gateway that fronts auth-service and stats-service. */
export const GATEWAY_URL = (import.meta.env.VITE_AUTH_URL || 'http://localhost:9080').replace(/\/$/, '')

// A public OIDC client of the "midtone" tenant (code + PKCE), using auth-service's hosted sign-in.
export const userManager = new UserManager({
  authority: `${GATEWAY_URL}/auth/t/midtone`,
  // assigned by auth-service when the application is registered
  client_id: import.meta.env.VITE_CLIENT_ID,
  redirect_uri: `${location.origin}/callback`,
  post_logout_redirect_uri: location.origin,
  scope: 'openid profile email offline_access',
})

/** The user's access token. With no usable session the browser is sent to sign in. */
export async function accessToken(): Promise<string> {
  let user = await userManager.getUser()
  if (user?.expired) user = await userManager.signinSilent().catch(() => null)
  if (user) return user.access_token
  await userManager.signinRedirect()
  // The page is navigating away; never resolve, so nothing renders half-authenticated.
  return new Promise(() => {})
}
