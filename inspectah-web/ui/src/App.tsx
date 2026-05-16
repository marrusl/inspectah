import {
  Page,
  PageSection,
  Content,
  Masthead,
  MastheadMain,
  MastheadBrand,
} from "@patternfly/react-core";

function App() {
  return (
    <Page
      masthead={
        <Masthead>
          <MastheadMain>
            <MastheadBrand>inspectah refine</MastheadBrand>
          </MastheadMain>
        </Masthead>
      }
    >
      <PageSection>
        <Content>
          <p>inspectah refine — loading...</p>
        </Content>
      </PageSection>
    </Page>
  );
}

export default App;
