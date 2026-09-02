local augur = require("augur")

local function failure(code, summary, steps)
  return { ok = false, code = code, summary = summary, steps = steps or {} }
end

local function run_repository(repo, settings)
  local steps = {}
  local ready = repo:wait_until_ready({ timeout_seconds = 5 * 60 })
  if not ready.ok then
    return failure(ready.code or "repository_busy", ready.summary or "repository stayed busy", steps)
  end
  local state = repo:status()
  if state.operation == "merge" then
    table.insert(steps, "recover existing merge")
    local result = repo:resolve_merge()
    if not result.ok then
      return failure(result.code or "merge_recovery_failed", result.summary or "merge recovery failed", steps)
    end
  elseif state.operation == "rebase" then
    table.insert(steps, "recover existing rebase")
    local result = repo:resolve_rebase()
    if not result.ok then
      return failure(result.code or "rebase_recovery_failed", result.summary or "rebase recovery failed", steps)
    end
  elseif state.operation ~= nil then
    return failure("unsupported_operation", "unsupported Git operation: " .. tostring(state.operation), steps)
  end

  state = repo:status()
  if state.conflicts then
    return failure("conflicts", "repository has unresolved conflicts", steps)
  end
  if state.dirty then
    table.insert(steps, "commit dirty worktree with AI")
    local result = repo:agent_commit({ hint = "Commit the current worktree as one concise Conventional Commit." })
    if not result.ok or not result.verified then
      return failure(result.code or "agent_commit_unverified", result.summary or "AI commit was not verified", steps)
    end
    state = repo:status()
    if state.dirty or state.conflicts then
      return failure("worktree_not_clean", "AI commit did not leave a clean worktree", steps)
    end
  end

  table.insert(steps, "pull --rebase")
  local pulled = repo:pull_rebase()
  if not pulled.ok then
    state = repo:status()
    if pulled.code == "conflict" or state.operation == "rebase" or state.conflicts then
      table.insert(steps, "recover pull rebase conflict with AI")
      local recovered = repo:resolve_rebase()
      if not recovered.ok or not recovered.verified then
        return failure(recovered.code or "rebase_recovery_unverified", recovered.summary or "rebase recovery was not verified", steps)
      end
      state = repo:status()
      if state.operation ~= nil or state.conflicts then
        return failure("rebase_state_unresolved", "rebase recovery left an unresolved state", steps)
      end
    else
      return failure(pulled.code or "pull_failed", pulled.summary or "pull --rebase failed", steps)
    end
  end

  table.insert(steps, "push")
  local pushed = repo:push({ remote = settings.default_remote })
  if not pushed.ok then
    return failure(pushed.code or "push_failed", pushed.summary or "push failed", steps)
  end
  return { ok = true, summary = "synchronized", steps = steps }
end

local function sync(ctx)
  local summary = {}
  local cancelled = false
  for _, repo in ipairs(ctx.repositories) do
    if ctx.cancelled() then
      cancelled = true
      break
    end
    local result = run_repository(repo, ctx.settings)
    result.repository = repo:display_name()
    table.insert(summary, result)
  end
  local failed = 0
  for _, result in ipairs(summary) do
    if not result.ok then
      failed = failed + 1
    end
  end
  augur.notify(failed == 0 and "info" or "warning", "Open tabs sync", string.format("%d repositories synchronized, %d failed", #summary - failed, failed))
  return {
    ok = not cancelled and failed == 0,
    code = cancelled and "cancelled" or nil,
    summary = cancelled and "sync cancelled" or nil,
    repositories = summary,
  }
end

return {
  on_run = sync,
  on_schedule = sync,
}
