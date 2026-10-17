// The production workflow's first job. It holds no GitHub environment and no secret, and every
// other job waits on it. GitHub creates an environment, unprotected, the first time a job names
// one that does not exist, so a production run must stop HERE until two things are true:
//  1. the production row of the environment table is deployable (the reviewed change removed its
//     placeholder), checked before any request;
//  2. `remote-production` already exists with its required reviewer, no administrator bypass, and
//     deployments from `main` and `v*` tags only. Reads only; prints no token and no response body.
import { pathToFileURL } from 'node:url'
import { deployable, requestBytes, requireValue, target } from './staging-common.mjs'

export const ENVIRONMENT = 'remote-production'
export const REVIEWER = 'kd9taw'
const BRANCHES = ['branch:main', 'tag:v*']

export function protectedEnvironment(environment, policies) {
  requireValue(environment?.name === ENVIRONMENT, `The GitHub environment read back is not ${ENVIRONMENT}`)
  requireValue(environment.can_admins_bypass === false,
    `${ENVIRONMENT} lets repository administrators bypass its reviewer; turn that off first`)
  const rules = Array.isArray(environment.protection_rules) ? environment.protection_rules : []
  requireValue(rules.some(rule => rule?.type === 'required_reviewers' && Array.isArray(rule.reviewers)
    && rule.reviewers.some(entry => entry?.type === 'User' && typeof entry.reviewer?.login === 'string'
      && entry.reviewer.login.toLowerCase() === REVIEWER)),
  `${ENVIRONMENT} does not require ${REVIEWER} to review each deployment`)
  const policy = environment.deployment_branch_policy
  requireValue(policy?.custom_branch_policies === true && policy.protected_branches === false,
    `${ENVIRONMENT} must deploy from selected branches and tags only`)
  const allowed = (Array.isArray(policies?.branch_policies) ? policies.branch_policies : [])
    .map(entry => `${entry?.type ?? 'branch'}:${entry?.name}`).sort()
  requireValue(JSON.stringify(allowed) === JSON.stringify(BRANCHES),
    `${ENVIRONMENT} must allow exactly the main branch and v* tags`)
  return { environment: ENVIRONMENT, reviewer: REVIEWER, adminBypass: false, deployFrom: allowed }
}

export async function productionGuard(env = process.env, fetcher = fetch) {
  const row = deployable(target(env))
  requireValue(row.target === 'production', 'The production workflow must target the production row')
  requireValue(env.GITHUB_REPOSITORY === 'kd9taw/Nexus', 'Production is administered only from kd9taw/Nexus')
  requireValue(typeof env.GITHUB_TOKEN === 'string' && env.GITHUB_TOKEN.length > 0 && !/\s/.test(env.GITHUB_TOKEN),
    'GITHUB_TOKEN is missing')
  const base = `https://api.github.com/repos/${env.GITHUB_REPOSITORY}/environments/${ENVIRONMENT}`
  const read = async (url, label) => {
    const response = await requestBytes(url, { headers: { accept: 'application/vnd.github+json',
      authorization: `Bearer ${env.GITHUB_TOKEN}`, 'user-agent': 'nexus-remote-production' } }, fetcher, label)
    requireValue(response.status !== 404, `${ENVIRONMENT} does not exist; create it with its reviewer before any production run`)
    requireValue(response.status === 200, `${label} failed (HTTP ${response.status})`)
    try { return JSON.parse(response.bytes.toString('utf8')) } catch { throw new Error(`${label} returned invalid JSON`) }
  }
  const environment = await read(base, `The ${ENVIRONMENT} environment read`)
  const policies = await read(`${base}/deployment-branch-policies`, `The ${ENVIRONMENT} branch policy read`)
  return { target: row.target, service: row.name, ...protectedEnvironment(environment, policies) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { console.log(JSON.stringify(await productionGuard())) }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
