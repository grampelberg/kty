# Metrics

- bytes_received_total - Total number of bytes received. This should be keyboard
  input.
- channel_bytes_sent_total - Total number of bytes sent via a channel by IO type
  (blocking, non-blocking). This is what the UI and raw modes use to send data
  to the client. It will be different that the amount of bytes `russh` itself.
- ssh_clients_total - Number of incoming connections.
- ssh_session_errors_total - Number of non-IO related unhandled errors at the
  session level.
- session_total - Number of sessions created.
- active_sessions - Number of currently active sessions.
- session_duration_minutes - Duration of a session in minutes.
- unexpected_state_total - Number of times an unexpected state was encountered.
  This should only be incremented if there's a bug.
- auth_attempts_total - Number of authentication attempts by method (publickey,
  interactive). This can seem inflated because `publickey` will always be
  attempted first and `interactive` will happen at least twice for every
  success.
- auth_results_total - Number of auth responses returned by method and result
  (accept, partial, reject). Note that this can seem inflated because
  `publickey` is always attempted first and provides a rejection before moving
  onto other methods.
- auth_succeeded_total - Number of fully authn and authz'd users. After this,
  users can request a PTY.
- code_generated_total - Number of codes generated for users. This is the first
  half of the `interactive` mode.
- code_checked_total - Number of codes that have been checked by result (valid,
  invalid). This is the second half of the `interactive` mode and it is possible
  that users retry after getting `invalid` because of something on the openid
  provider side.
- container_exec_duration_minutes - Number of minutes a raw terminal was running
  exec'd into a pod.
- table_filter_total - Number of times a table was filtered.
- widget_views_total - Number of times a widget was created by resource
  (container, pod) and type (cmd, log, yaml, ...).
- requests_total - Number of requests that have come in by type (pty, sftp,
  window_resize).
- sftp_active_sessions - Total number of active sessions currently.
- sftp_bytes_total - Total number of bytes transferred via sftp by direction
  (read, write).
- sftp_files_total - Total number of files by direction (sent, received).
- sftp_stat_total - Total number of times `stat` was called on a path.
- sftp_list_total - Total number of times `list` was called on a path.
- channels_total - Total number of channel actions by method (open_session,
  direct_tcpip, ...).
- stream_duration_seconds - Number of seconds a stream was alive by resource and
  direction.
- stream_bytes_total - Number of bytes transfered by resource, direction and
  destination.
- stream_total - Total number of streams by resource and direction.
- stream_active - Currently active numberof streams by resource and direction.
