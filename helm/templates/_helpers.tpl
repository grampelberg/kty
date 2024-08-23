{{- define "labels" -}}
app.kubernetes.io/name: {{ .name | replace "-" "" }}
app.kubernetes.io/component: {{ .component }}
app.kubernetes.io/part-of: {{ .global.Chart.Name }}
app.kubernetes.io/instance: {{ .global.Release.Name }}
app.kubernetes.io/managed-by: {{ .global.Release.Service }}
{{- end }}

{{- define "image" -}}
{{ .image | default (printf "ghcr.io/grampelberg/kuberift:%s" .global.Chart.AppVersion) }}
{{- end }}
