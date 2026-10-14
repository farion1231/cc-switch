import type { ProviderFormProps } from "./ProviderForm";
import { PiProviderForm } from "./PiProviderForm";

export function OmpProviderForm(props: ProviderFormProps) {
  return <PiProviderForm {...props} appId="omp" />;
}
